//! Revocation admission and readmission share the enrollment's journal reference.

use super::*;
use durable_steps::{
    CallTarget, JournalScope, OperationContext, OperationFailure, OperationId, RequestKey, Task,
    storage::JournalStorage,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
const REVOKE_TASK_KIND: &str = "central.revoke-machine";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndPeerSessions {
    pub peer: PeerId,
}
impl CallTarget for EndPeerSessions {
    const KIND: &'static str = "central.end-peer-sessions";
    type Request = ();
    type Answer = ();
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RevokeInput {
    pub peer: PeerId,
}
pub(crate) struct RevokeMachineTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for RevokeMachineTask<Store> {
    const KIND: &'static str = REVOKE_TASK_KIND;
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = RevokeInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &RevokeInput,
    ) -> Result<(), OperationFailure> {
        context
            .call(
                durable_steps::StepName::new("end_sessions").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                EndPeerSessions { peer: input.peer },
                (),
            )
            .await?;
        context
            .call(
                durable_steps::StepName::new("close_mailbox").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                crate::mailbox_lifecycle::ClosePeerMailbox { peer: input.peer },
                (),
            )
            .await?;
        Ok(())
    }
}

pub(super) async fn in_progress<Scope: JournalStorage>(
    scope: &mut Scope,
    operation: Option<OperationId>,
) -> Result<bool, StoreError> {
    operation_pending(
        scope,
        operation,
        REVOKE_TASK_KIND,
        "last_revocation_operation",
    )
    .await
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
    pub async fn revoke_machine(
        &self,
        _owner: &OwnerIdentity,
        installation: ServiceId,
    ) -> Result<RevocationDecision, StoreError> {
        let ObjectWork::Registry {
            host,
            shared_host,
            engine,
            ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
        let (admission, _) = StepDriver::new(host.clone())
            .run(RevokeStep {
                installation,
                nonce: shared_host.new_id(),
                now: shared_host.now(),
            })
            .await?;
        match admission {
            RevokeAdmission::EnrollmentInProgress => {
                return Ok(RevocationDecision::EnrollmentInProgress);
            }
            RevokeAdmission::Pending => {
                engine.drive_due().await?;
            }
            RevokeAdmission::Finished => {}
        }
        Ok(RevocationDecision::MachineRevoked)
    }
}

struct RevokeStep {
    installation: ServiceId,
    nonce: durable_steps::Uuid,
    now: SyncTime,
}
enum RevokeAdmission {
    Finished,
    Pending,
    EnrollmentInProgress,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for RevokeStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RevokeAdmission;
    type Effect = Infallible;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut rows = queries::read_enrollments(
                scope,
                Some(self.installation.as_str().into()),
                None,
                None,
                None,
            )
            .await
            .map_err(StoreError::backend)?;
            if rows.len() > 1 {
                return Err(corrupt("service_id"));
            }
            let Some(machine) = rows.pop().map(StoredMachine::decode).transpose()? else {
                return Ok((RevokeAdmission::Finished, Vec::new()));
            };
            if machine.state != EnrollmentState::Enrolled
                && operation_pending(
                    scope,
                    machine.last_enrollment_operation,
                    readmission::READMISSION_TASK_KIND,
                    "last_enrollment_operation",
                )
                .await?
            {
                return Ok((RevokeAdmission::EnrollmentInProgress, Vec::new()));
            }
            if machine.state == EnrollmentState::Revoked {
                return Ok((
                    if machine.last_revocation_operation.is_some() {
                        RevokeAdmission::Pending
                    } else {
                        RevokeAdmission::Finished
                    },
                    Vec::new(),
                ));
            }
            let key = RequestKey::new(format!("central-revoke:{}:{}", machine.peer, self.nonce))
                .map_err(StoreError::backend)?;
            let admitted = scope
                .admit::<RevokeMachineTask<Store>>(&key, &RevokeInput { peer: machine.peer })
                .await
                .map_err(StoreError::backend)?;
            let operation = admitted.id();
            queries::revoke_enrollment(scope, String::from(operation), machine.peer.to_string())
                .await
                .map_err(StoreError::backend)?;
            scope.ensure_wake(self.now).await?;
            Ok((RevokeAdmission::Pending, Vec::new()))
        })
    }
}
