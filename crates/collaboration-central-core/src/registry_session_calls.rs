//! Session publication and Welcome reads use the registry's real shared rules.

use crate::{
    CallerKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    registry_peer_lookup::service_for_peer,
    session_calls::WelcomeLeases,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::{LeaseStatus, PeerId, ReportSchemaSupport};
use fleet_sync_session::SchemaSupportRegistry;
use message_board::ServiceId;
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

enum SessionRegistryRequest {
    Report(ReportSchemaSupport),
    Welcome(PeerId),
}
enum SessionRegistryReply {
    Report(fleet_sync_protocol::SchemaSupportRecorded),
    Welcome(Vec<LeaseStatus>),
    Refused,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn session_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectAddress::Connection(service) = invocation.source else {
            return Err(refused());
        };
        let request = match invocation.kind.as_str() {
            SchemaSupportRegistry::KIND => {
                let _: SchemaSupportRegistry =
                    serde_json::from_value(invocation.target).map_err(|_| refused())?;
                SessionRegistryRequest::Report(
                    serde_json::from_value(invocation.request).map_err(|_| refused())?,
                )
            }
            WelcomeLeases::KIND => {
                let _: WelcomeLeases =
                    serde_json::from_value(invocation.target).map_err(|_| refused())?;
                SessionRegistryRequest::Welcome(
                    serde_json::from_value(invocation.request).map_err(|_| refused())?,
                )
            }
            _ => return Err(refused()),
        };
        let ObjectWork::Registry {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (reply, _) = StepDriver::new(host.clone())
            .run(SessionRegistryStep {
                service,
                request,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        match reply {
            SessionRegistryReply::Report(recorded) => serde_json::to_vec(&recorded),
            SessionRegistryReply::Welcome(leases) => serde_json::to_vec(&leases),
            SessionRegistryReply::Refused => return Err(refused()),
        }
        .map_err(|_| CallFailure::AnswerLost)
    }

    /// Read the committed library projection without changing session or registry state.
    #[cfg(feature = "conformance")]
    pub async fn conformance_schema_report(
        &self,
        peer: PeerId,
    ) -> Result<Option<ReportSchemaSupport>, StoreError> {
        let ObjectWork::Registry { host, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "not a registry runtime".into(),
            });
        };
        let (report, _) = StepDriver::new(host.clone())
            .run(ReadSchemaReport { peer })
            .await?;
        Ok(report)
    }
}

#[cfg(feature = "conformance")]
struct ReadSchemaReport {
    peer: PeerId,
}
#[cfg(feature = "conformance")]
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadSchemaReport
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<ReportSchemaSupport>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let report = fleet_sync_lease::accepted_schema_support(scope, self.peer)
                .await
                .map_err(StoreError::backend)?;
            Ok((report, Vec::new()))
        })
    }
}

struct SessionRegistryStep {
    service: ServiceId,
    request: SessionRegistryRequest,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for SessionRegistryStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = SessionRegistryReply;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let peer = match &self.request {
                SessionRegistryRequest::Report(report) => report.peer,
                SessionRegistryRequest::Welcome(peer) => *peer,
            };
            if service_for_peer(scope, peer).await?.as_ref() != Some(&self.service) {
                return Ok((SessionRegistryReply::Refused, Vec::new()));
            }
            let reply = match self.request {
                SessionRegistryRequest::Report(report) => {
                    let previous = fleet_sync_lease::accepted_schema_support(scope, peer)
                        .await
                        .map_err(StoreError::backend)?;
                    let newly_admitted = previous
                        .as_ref()
                        .is_none_or(|previous| previous.attach_seq < report.attach_seq);
                    let answer = fleet_sync_lease::report_schema_support(scope, &report)
                        .await
                        .map_err(StoreError::backend)?;
                    if newly_admitted {
                        let at = chrono::DateTime::from_timestamp_millis(self.now.unix_millis())
                            .ok_or_else(|| StoreError::Corrupt {
                                field: "machine_enrollments.last_seen".into(),
                                message: "Registry clock is outside the stored time range".into(),
                            })?;
                        crate::generated_enrollment::record_machine_presence(
                            scope,
                            at.to_rfc3339(),
                            peer.to_string(),
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    }
                    SessionRegistryReply::Report(answer)
                }
                SessionRegistryRequest::Welcome(peer) => {
                    let mut leases = Vec::new();
                    for database in fleet_sync_lease::database_ids(scope)
                        .await
                        .map_err(StoreError::backend)?
                    {
                        match fleet_sync_lease::lease_status(scope, database, self.now).await {
                            Ok(Some(lease)) if lease.holder == peer => leases.push(lease),
                            Ok(_) | Err(fleet_sync_lease::LeaseError::Retired) => {}
                            Err(error) => return Err(StoreError::backend(error)),
                        }
                    }
                    SessionRegistryReply::Welcome(leases)
                }
            };
            Ok((reply, Vec::new()))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
