//! One journaled append owns the machine position; session notices carry its live hint.
use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use board_sync_protocol::{BoardConsumerProtocol, BoardNotice};
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
pub(crate) struct MachineNoticeTask<Host, Io>(pub PhantomData<fn() -> (Host, Io)>);
impl<Host: ObjectHost, Io: PlatformIo> Task for MachineNoticeTask<Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    const KIND: &'static str = "central.machine-notice-append";
    const VERSION: u32 = 1;
    type Store = CoreSessionStore<Host, Io>;
    type Input = BoardNotice;
    type Output = ();
    async fn run<Context: OperationContext<Store = Self::Store>>(
        &self,
        context: &mut Context,
        input: &BoardNotice,
    ) -> Result<(), OperationFailure> {
        // One receiving operation is one notice. Its durable v7 identity is also
        // the private row key, so retries never mint another machine notice.
        let notice_id = String::from(*context.id());
        let kind = input.clone();
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
                        let (notice, peer) =
                            crate::machine_notice_store::append(scope, notice_id, kind, now)
                                .await?;
                        if let Some(peer) = peer {
                            let effects =
                                fleet_sync_session::publish_notice_to_peer::<
                                    _,
                                    BoardConsumerProtocol,
                                >(
                                    scope, &peer, fleet_sync_protocol::Notice::Consumer(notice)
                                )
                                .await
                                .map_err(StoreError::backend)?;
                            scope.stage_transport(effects, None)?;
                        }
                        if let Some(at) = crate::machine_notice_store::next_expiry(scope).await? {
                            scope.ensure_wake(at).await?;
                        }
                        Ok(())
                    })
                },
            )
            .await?;
        Ok(())
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<DeliveryKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
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
                (CallerKind::Registry, ObjectAddress::Registry)
                    | (CallerKind::Fleet, ObjectAddress::Fleet)
                    | (CallerKind::Mailbox, ObjectAddress::Mailbox(_))
            )
        {
            return Err(CallFailure::AnswerLost);
        }
        let target: DeliverMachineNotice =
            serde_json::from_value(invocation.target).map_err(encode_refusal)?;
        let notice: BoardNotice =
            serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        if let ObjectAddress::Mailbox(source) = &invocation.source
            && (source != &target.installation
                || !matches!(notice, BoardNotice::MailWaiting { .. }))
        {
            return Err(CallFailure::AnswerLost);
        }
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            installation,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        if target.installation != *installation {
            return Err(CallFailure::AnswerLost);
        }
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
                    let admitted = scope
                        .admit::<MachineNoticeTask<Host, Io>>(&key, &notice)
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
