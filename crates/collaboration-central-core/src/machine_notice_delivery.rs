//! One journaled append owns the machine position; session notices carry its live hint.
use crate::machine_notice_contract::MachineNoticeAppend;
use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectPlatform, ObjectRuntime,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use board_sync_protocol::BoardNotice;
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, JournalScope, OperationContext, OperationFailure,
    ReborrowScope, RequestKey, StoreError, Task,
    storage::{JournalStorage, OperationExecution, TransactionScope},
};
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliverMachineNotice {
    pub installation: message_board::ServiceId,
}
impl CallTarget for DeliverMachineNotice {
    const KIND: &'static str = "central.machine-notice";
    type Request = BoardNotice;
    type Answer = ();
}
pub(crate) struct MachineNoticeTask<Host>(pub PhantomData<fn() -> Host>);
impl<Host: ObjectPlatform> Task for MachineNoticeTask<Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    const KIND: &'static str = "central.machine-notice-append";
    const VERSION: u32 = 1;
    type Store = CoreSessionStore<Host>;
    type Input = MachineNoticeAppend;
    type Output = ();
    async fn run<Context: OperationContext<Store = Self::Store>>(
        &self,
        context: &mut Context,
        input: &MachineNoticeAppend,
    ) -> Result<(), OperationFailure> {
        // One receiving operation is one notice. Its durable v7 identity is also
        // the private row key, so retries never mint another machine notice.
        let notice_id = String::from(*context.id());
        let input = match input {
            MachineNoticeAppend::Append { peer, notice } => MachineNoticeAppend::Append {
                peer: *peer,
                notice: notice.clone(),
            },
            MachineNoticeAppend::Hint { peer, notice } => MachineNoticeAppend::Hint {
                peer: *peer,
                notice: notice.clone(),
            },
        };
        let now = context.now();
        context
            .local(
                durable_steps::StepName::new("append_notice").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                move |scope| {
                    Box::pin(async move {
                        // Append only: the Fleet's notice push sends it after commit
                        // (slice 4a). A Hint was a live send of an appended notice, so
                        // the push covers it too.
                        if let MachineNoticeAppend::Append { peer, notice } = input {
                            crate::machine_notice_store::append(
                                scope, peer, notice_id, notice, now,
                            )
                            .await?;
                        }
                        Ok(())
                    })
                },
            )
            .await?;
        Ok(())
    }
}

impl<Host: ObjectPlatform> ObjectRuntime<DeliveryKind, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn receive_machine_notice(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if !matches!(invocation.authority, CallAuthority::Object)
            || !matches!(
                (invocation.source_face, &invocation.source),
                (CallerKind::Registry, ObjectAddress::Fleet)
                    | (CallerKind::Fleet, ObjectAddress::Fleet)
            )
        {
            return Err(CallFailure::AnswerLost);
        }
        let target: DeliverMachineNotice =
            serde_json::from_value(invocation.target).map_err(encode_refusal)?;
        let notice: BoardNotice =
            serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        let ObjectWork::Fleet {
            host,
            shared_host,
            replies,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let key = RequestKey::new(format!(
            "central-machine-notice:{}:{}",
            String::from(invocation.identity.operation),
            invocation.identity.step.as_str()
        ))
        .map_err(encode_refusal)?;
        let now = shared_host.now();
        let store = Arc::new(CoreSessionStore::new(
            host.clone(),
            shared_host.clone(),
            replies.clone(),
        ));
        let completed = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut enrolled = crate::generated_enrollment::read_enrollments(
                        scope,
                        Some(target.installation.as_str().to_owned()),
                        None,
                        None,
                        None,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    if enrolled.len() != 1 {
                        return Err(StoreError::Aborted {
                            reason: "control notice recipient is not enrolled".into(),
                        });
                    }
                    let identity =
                        crate::machine_enrollment_record::identity_from_stored_enrollment(
                            enrolled
                                .pop()
                                .ok_or_else(|| StoreError::backend("missing notice recipient"))?,
                        )?;
                    let input = MachineNoticeAppend::Append {
                        peer: identity.peer,
                        notice,
                    };
                    let admitted = scope
                        .admit::<MachineNoticeTask<Host>>(&key, &input)
                        .await
                        .map_err(StoreError::backend)?;
                    let operation = scope
                        .operation(admitted.id())
                        .await?
                        .ok_or(StoreError::MissingOperation { id: admitted.id() })?;
                    match operation.execution {
                        OperationExecution::Completed { result, .. } => {
                            let (): () =
                                serde_json::from_slice(&result).map_err(StoreError::backend)?;
                            Ok(true)
                        }
                        OperationExecution::Running { .. } => {
                            scope.ensure_wake(now).await?;
                            Ok(false)
                        }
                        OperationExecution::Failed { .. } => Err(StoreError::Aborted {
                            reason: "machine notice append failed".into(),
                        }),
                    }
                })
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if completed {
            serde_json::to_vec(&()).map_err(|_| CallFailure::AnswerLost)
        } else {
            Err(CallFailure::AnswerLost)
        }
    }
}
