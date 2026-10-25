//! The connection's existing journal makes old revocation calls harmless on replay.

use crate::{
    DeliveryKind, ObjectHost, ObjectRuntime, PlatformIo,
    machine_enrollment::EndPeerSessions,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use durable_steps::{
    AtomicStore, CallFailure, JournalScope, OperationContext, OperationFailure, ReborrowScope,
    RequestKey, StoreError, Task,
    storage::{JournalStorage, OperationExecution, TransactionScope},
};
use fleet_sync_protocol::PeerId;
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndSessionsInput {
    peer: PeerId,
}

pub(crate) struct EndSessionsTask<Host, Io>(pub PhantomData<fn() -> (Host, Io)>);
impl<Host: ObjectHost, Io: PlatformIo> Task for EndSessionsTask<Host, Io>
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
    const KIND: &'static str = "central.end-sessions";
    const VERSION: u32 = 1;
    type Store = CoreSessionStore<Host, Io>;
    type Input = EndSessionsInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Self::Store>>(
        &self,
        context: &mut Context,
        input: &EndSessionsInput,
    ) -> Result<(), OperationFailure> {
        let peer = input.peer;
        let now = context.now();
        context
            .local(
                durable_steps::StepName::new("end_peer").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                move |scope| {
                    Box::pin(async move {
                        let effects = fleet_sync_session::end_sessions::<
                            _,
                            board_sync_protocol::BoardConsumerProtocol,
                        >(
                            scope,
                            &peer,
                            fleet_sync_session::EndReason::PeerRevoked,
                            now,
                        )
                        .await
                        .map_err(StoreError::backend)?;
                        scope.stage_transport(effects, None)?;
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
    pub(crate) async fn end_peer_sessions(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        let target: EndPeerSessions =
            serde_json::from_value(invocation.target).map_err(encode_refusal)?;
        let (): () = serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let key = RequestKey::new(format!(
            "central-end-sessions:{}:{}",
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
                        .admit::<EndSessionsTask<Host, Io>>(
                            &key,
                            &EndSessionsInput { peer: target.peer },
                        )
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
                            reason: "session revocation failed".into(),
                        }),
                    }
                })
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if completed {
            serde_json::to_vec(&()).map_err(|_| CallFailure::AnswerLost)
        } else {
            // Admission may already have ended the session before this answer
            // arrives. Only the saved completion can acknowledge the call.
            Err(CallFailure::AnswerLost)
        }
    }
}
