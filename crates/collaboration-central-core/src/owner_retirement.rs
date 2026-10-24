//! The library commits retirement; existing alarm publication owns Directory.
use crate::{
    ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    object_runtime::ObjectWork,
    owner_registry_calls::{
        OwnerBlockedSubject, OwnerRetirement, OwnerRetirementAnswer, OwnerRetirementPhase,
    },
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, OperationId, OperationState, StartRefusal,
    StoreError,
    storage::{JournalStorage, StepState, TransactionScope},
};
use fleet_sync_lease::{DatabaseLifecycle, RetirementTask};
use fleet_sync_protocol::{
    CopyUnavailableReason, DatabaseId, Retirement, RetirementAnswer, SyncTime, TransferId,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

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
    pub(crate) async fn owner_retire(
        &self,
        database: DatabaseId,
        request: OwnerRetirement,
    ) -> Result<OwnerRetirementAnswer, CallFailure> {
        let ObjectWork::Registry { engine, host, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let attempt = request.request_id;
        let key = fleet_sync_lease::lifecycle_request_key(attempt)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        // Always bind the request before consulting current lifecycle. A final
        // tombstone cannot make changed content under the same ID acceptable.
        let operation = match engine
            .start::<RetirementTask<Host::Store, Host::Database>>(
                &key,
                Retirement {
                    attempt,
                    database,
                    expect_fence: request.expect_fence,
                    expect_acked: request.expect_acked,
                },
            )
            .await
        {
            Ok(operation) => operation,
            Err(StartRefusal::RequestConflict { .. }) => {
                return Ok(OwnerRetirementAnswer::ConflictingRequest);
            }
            Err(_) => return Err(CallFailure::AnswerLost),
        };
        engine
            .drive_due()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let state = engine
            .status(&operation)
            .await
            .map_err(|_| CallFailure::AnswerLost)?
            .state;
        if let OperationState::Completed { result } = &state {
            return match serde_json::from_slice::<RetirementAnswer>(result)
                .map_err(|_| CallFailure::AnswerLost)?
            {
                RetirementAnswer::Refused(reason) => Ok(OwnerRetirementAnswer::Refused(reason)),
                RetirementAnswer::Retired { at } => {
                    self.retirement_directory_result(database, attempt, at)
                        .await
                }
            };
        }
        let (waiting, _) = StepDriver::new(host.clone())
            .run(ReadRetirementWait {
                database,
                operation,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        // The public library tombstone remains authoritative even if a crash
        // interrupted the outer journal after that commit. Publication resumes.
        if let Some(DatabaseLifecycle::Retired { at }) = waiting.lifecycle {
            return self
                .retirement_directory_result(database, attempt, at)
                .await;
        }
        if matches!(state, OperationState::Failed { .. }) {
            return Err(CallFailure::AnswerLost);
        }
        let Some(lifecycle) = waiting.lifecycle else {
            return Ok(OwnerRetirementAnswer::NotFound);
        };
        if let Some(reason) = waiting.copy_unavailable {
            // Retirement's request identity is its transfer identity (§11).
            let transfer = TransferId::new(attempt.as_uuid())
                .map_err(crate::object_call_envelope::encode_refusal)?;
            return Ok(OwnerRetirementAnswer::Unavailable {
                subject: OwnerBlockedSubject::Transfer { transfer },
                reason,
            });
        }
        let phase = match lifecycle {
            DatabaseLifecycle::Retiring { .. } => OwnerRetirementPhase::Wiping,
            DatabaseLifecycle::Registered { .. } | DatabaseLifecycle::Unheld { .. } => {
                OwnerRetirementPhase::Checking
            }
            DatabaseLifecycle::Retired { .. } => return Err(CallFailure::AnswerLost),
        };
        Ok(OwnerRetirementAnswer::Pending {
            request_id: attempt,
            phase,
        })
    }
    async fn retirement_directory_result(
        &self,
        database: DatabaseId,
        request_id: fleet_sync_protocol::RequestId,
        at: SyncTime,
    ) -> Result<OwnerRetirementAnswer, CallFailure> {
        let ObjectWork::Registry { shared_host, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        shared_host
            .request_lease_publication_repair()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if shared_host
            .retirement_directory_acknowledged(database, at)
            .await
            .map_err(|_| CallFailure::AnswerLost)?
        {
            Ok(OwnerRetirementAnswer::Retired { at })
        } else {
            Ok(OwnerRetirementAnswer::Pending {
                request_id,
                phase: OwnerRetirementPhase::Directory,
            })
        }
    }
}

struct ReadRetirementWait {
    database: DatabaseId,
    operation: OperationId,
}
struct RetirementWait {
    lifecycle: Option<DatabaseLifecycle>,
    copy_unavailable: Option<CopyUnavailableReason>,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadRetirementWait
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RetirementWait;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let lifecycle = fleet_sync_lease::database_lifecycle(scope, self.database)
                .await
                .map_err(StoreError::backend)?;
            let mut copy_unavailable = None;
            for position in 0..scope.step_count(self.operation).await? {
                let step = scope
                    .step(self.operation, position)
                    .await?
                    .ok_or_else(|| StoreError::backend("retirement journal step missing"))?;
                if let StepState::CallPending { intent, attempt } = step.state
                    && let Some(reason) = attempt.retry_reason
                {
                    let (kind, _): (String, serde_json::Value) =
                        serde_json::from_str(&intent.target).map_err(StoreError::backend)?;
                    if kind == fleet_sync_lease::CopyRetire::KIND {
                        copy_unavailable = Some(
                            CopyUnavailableReason::decode_retry(&reason.code, &reason.detail)
                                .map_err(StoreError::backend)?,
                        );
                    }
                }
            }
            Ok((
                RetirementWait {
                    lifecycle,
                    copy_unavailable,
                },
                Vec::new(),
            ))
        })
    }
}
