//! Authenticated session lease requests use the library's registry rules.
use crate::FleetKind;

use crate::{
    CallerKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    registry_peer_lookup::service_for_peer,
    session_calls::{SessionLeaseRegistry, SessionLeaseRequest},
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, NotSentReason, OperationState, StoreError,
    storage::TransactionScope,
};
use fleet_sync_lease::{DatabaseLifecycle, LeasePolicy, RegistrationTask, lifecycle_request_key};
use fleet_sync_protocol::{
    ActorRef, DatabaseId, HolderChangeAnswer, HolderChangeRefusal, HolderChangeRequest,
    LeaseAnswer, LeaseRequest, RegistrationAnswer, RegistrationRefusal, RenewRequest, TransferId,
};
use fleet_sync_session::SessionKey;
use message_board::ServiceId;
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    Host::Store: durable_steps::ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn lease_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(refused());
        }
        let CallAuthority::Machine {
            installation: service,
        } = invocation.authority
        else {
            return Err(refused());
        };
        let _: SessionLeaseRegistry =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let SessionLeaseRequest { session, request } =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            engine,
            host,
            shared_host,
            io,
            config,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let database = match &request {
            LeaseRequest::Register(request) => request.database,
            LeaseRequest::Renew(request) => request.database,
            LeaseRequest::Claim { database, .. } => *database,
        };
        let (caller, _) = StepDriver::new(host.clone())
            .run(ReadLeaseCaller {
                service: service.clone(),
                session,
                database,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let caller = caller.ok_or_else(refused)?;
        let reply = match request {
            LeaseRequest::Register(request) if request.writer != session.peer => {
                LeaseAnswer::Register(RegistrationAnswer::Refused(
                    RegistrationRefusal::NotWriter { current: None },
                ))
            }
            LeaseRequest::Register(_)
                if matches!(
                    caller.lifecycle,
                    Some(DatabaseLifecycle::Retired { .. } | DatabaseLifecycle::Retiring { .. })
                ) =>
            {
                // Current retirement outranks an engine's cached registration.
                LeaseAnswer::Register(RegistrationAnswer::Refused(RegistrationRefusal::Retired))
            }
            LeaseRequest::Register(request) => {
                let key = lifecycle_request_key(request.attempt).map_err(|_| refused())?;
                // start owns admission and immediate driving; it is outside
                // the committed caller/lifecycle read section above.
                let operation = engine
                    .start_registration(&key, request)
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                engine
                    .drive_due()
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                match engine
                    .status(&operation)
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?
                    .state
                {
                    OperationState::Completed { result } => LeaseAnswer::Register(
                        serde_json::from_slice(&result).map_err(|_| CallFailure::AnswerLost)?,
                    ),
                    // There is no registration Pending wire answer. Losing
                    // this transport answer leaves the same attempt retryable.
                    OperationState::Running { .. } | OperationState::Failed { .. } => {
                        return Err(CallFailure::AnswerLost);
                    }
                }
            }
            LeaseRequest::Renew(request) => {
                let now = shared_host.now();
                let policy = config.lease;
                let hook = crate::fleet_lease_projection_step::FleetLeaseChangeHook::<
                    crate::session_store::CoreSessionStore<Host, Io>,
                    crate::machine_notice_delivery::MachineNoticeTask<Host, Io>,
                >::new(io.clone());
                let (store, _) = self.session_store().map_err(|_| CallFailure::AnswerLost)?;
                let reply = store
                    .transaction(move |scope| {
                        Box::pin(async move {
                            if service_for_peer(scope, session.peer).await?.as_ref()
                                != Some(&service)
                            {
                                return Ok(None);
                            }
                            let answer = fleet_sync_lease::renew(
                                scope,
                                &request,
                                &session.peer,
                                now,
                                &policy,
                                &hook,
                            )
                            .await
                            .map_err(StoreError::backend)?;
                            Ok(Some(LeaseAnswer::Renew(answer)))
                        })
                    })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                reply.ok_or_else(refused)?
            }
            LeaseRequest::Claim { .. }
                if matches!(caller.lifecycle, Some(DatabaseLifecycle::Retired { .. })) =>
            {
                LeaseAnswer::Claim(HolderChangeAnswer::Refused(HolderChangeRefusal::Retired))
            }
            LeaseRequest::Claim { attempt, database } => {
                let transfer = TransferId::new(attempt.as_uuid()).map_err(|_| refused())?;
                let request = HolderChangeRequest::Claim {
                    attempt: transfer,
                    database,
                    claimant: session.peer,
                };
                let actor = ActorRef::new(format!("peer:{}", session.peer.as_uuid()))
                    .map_err(|_| refused())?;
                let now = shared_host.now();
                StepDriver::new(host.clone())
                    .run(AdmitClaim {
                        request: request.clone(),
                        actor,
                        now,
                    })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                // The library replays an earlier refusal before checking Retiring.
                // Admission commits before network drive; its canonical domain
                // answer also preserves a grant when later settlement is pending.
                engine
                    .drive_due()
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                let (answer, _) = StepDriver::new(host.clone())
                    .run(ReadClaimAnswer { request })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                LeaseAnswer::Claim(answer.unwrap_or(HolderChangeAnswer::Pending { transfer }))
            }
        };
        serde_json::to_vec(&reply).map_err(|_| CallFailure::AnswerLost)
    }
}

struct LeaseCallerState {
    lifecycle: Option<DatabaseLifecycle>,
}
struct ReadLeaseCaller {
    service: ServiceId,
    session: SessionKey,
    database: DatabaseId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadLeaseCaller
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<LeaseCallerState>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            if service_for_peer(scope, self.session.peer).await?.as_ref() != Some(&self.service) {
                return Ok((None, Vec::new()));
            }
            let lifecycle = fleet_sync_lease::database_lifecycle(scope, self.database)
                .await
                .map_err(StoreError::backend)?;
            Ok((Some(LeaseCallerState { lifecycle }), Vec::new()))
        })
    }
}

struct AdmitClaim {
    request: HolderChangeRequest,
    actor: ActorRef,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitClaim
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<(), Self::Effect>> {
        Box::pin(async move {
            fleet_sync_lease::admit_holder_change::<Store, Driver>(
                scope,
                &self.request,
                &self.actor,
                self.now,
            )
            .await
            .map_err(StoreError::backend)?;
            Ok(((), Vec::new()))
        })
    }
}
struct ReadClaimAnswer {
    request: HolderChangeRequest,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadClaimAnswer
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<HolderChangeAnswer>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            Ok((
                fleet_sync_lease::holder_change_answer(scope, &self.request)
                    .await
                    .map_err(StoreError::backend)?,
                Vec::new(),
            ))
        })
    }
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
