//! Delivery accepts verified object callers; opaque replies travel only after commit.
use crate::{
    CallerKind, DeliveryKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    forward_calls::{
        AnswerForward, DeliverForward, ForwardAnswerRequest, ForwardDeliveryRequest,
        ForwardDeliveryResult,
    },
    forward_store::{self, DeliveredForward, ForwardPhase},
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    session_store::{CoreSessionStore, SessionTransportEffect},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardServerRequest, ForwardOutcome};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, JournalScope, NotSentReason, ReborrowScope, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::{ServerFrame, ServerRequest};
use fleet_sync_session::{ConnectionRegistration, SessionKey};
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<DeliveryKind, Host, Io>
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
    pub(crate) async fn receive_forward_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(unsent());
        }
        let CallAuthority::Machine {
            installation: source,
        } = invocation.authority
        else {
            return Err(unsent());
        };
        let ObjectWork::Fleet {
            host,
            shared_host,
            replies,
            ..
        } = &self.work
        else {
            return Err(unsent());
        };
        let store = CoreSessionStore::new(host.clone(), shared_host.clone(), replies.clone());
        match invocation.kind.as_str() {
            DeliverForward::KIND => {
                let _target: DeliverForward =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let request: ForwardDeliveryRequest =
                    serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                let selection = host.clone();
                let clock = shared_host.clone();
                let result = store
                    .transaction(move |scope| {
                        Box::pin(async move {
                            let now = clock.now();
                            if request.deadline <= now {
                                return Err(StoreError::Aborted {
                                    reason: "forward delivery deadline elapsed".into(),
                                });
                            }
                            if let Some(existing) =
                                forward_store::delivered_by_frame(scope, request.forward_frame)
                                    .await?
                            {
                                if existing.sender != source
                                    || existing.project != request.post.project
                                    || existing.deadline != request.deadline
                                    || existing.holder != request.holder
                                    || existing.origin_session != request.origin_session
                                    || existing.request_frame != request.request_frame
                                    || existing.connection != request.connection
                                    || existing.created_at != request.created_at
                                {
                                    return Err(StoreError::Corrupt {
                                        field: "delivered_forwards.forward_frame".into(),
                                        message: "correlation identity was reused".into(),
                                    });
                                }
                                return Ok(ForwardDeliveryResult::Delivered);
                            }
                            if crate::machine_enrollment::admission::read_current_enrolled_machine(
                                scope,
                                request.holder,
                                now,
                            )
                            .await?
                            .is_none()
                            {
                                return Ok(ForwardDeliveryResult::NoConnection);
                            }
                            for (connection, registration) in scope.registrations.clone() {
                                let ConnectionRegistration::Attached { session_id, .. } =
                                    registration
                                else {
                                    continue;
                                };
                                let Ok(binding) = selection.admission_binding(connection).await
                                else {
                                    continue;
                                };
                                if binding.connection != connection
                                    || binding.peer != request.holder
                                    || binding.expires_at <= now
                                {
                                    continue;
                                }
                                let session = SessionKey {
                                    peer: binding.peer,
                                    session_id,
                                };
                                forward_store::save_delivered(
                                    scope,
                                    &DeliveredForward {
                                        forward_frame: request.forward_frame,
                                        sender: source,
                                        project: request.post.project.clone(),
                                        holder: binding.peer,
                                        deadline: request.deadline,
                                        origin_session: request.origin_session,
                                        request_frame: request.request_frame,
                                        connection: request.connection,
                                        created_at: request.created_at,
                                        updated_at: now,
                                    },
                                )
                                .await?;
                                scope.ensure_wake(request.deadline).await?;
                                let frame = ServerFrame::<BoardConsumerProtocol>::Request {
                                    frame: request.forward_frame,
                                    body: ServerRequest::Consumer(
                                        BoardServerRequest::ForwardedPost(request.post),
                                    ),
                                };
                                scope.transport.push(SessionTransportEffect::ForwardFrame {
                                    connection,
                                    session,
                                    bytes: serde_json::to_vec(&frame)
                                        .map_err(StoreError::backend)?
                                        .into(),
                                });
                                return Ok(ForwardDeliveryResult::Delivered);
                            }
                            Ok(ForwardDeliveryResult::NoConnection)
                        })
                    })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
            }
            AnswerForward::KIND => {
                let _target: AnswerForward =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let request: ForwardAnswerRequest =
                    serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                let clock = shared_host.clone();
                store.transaction(move |scope|Box::pin(async move {
                    let Some(pending)=forward_store::pending_by_frame(scope,request.forward_frame).await? else {return Ok(());};
                    if pending.deadline<=clock.now() || pending.project!=request.project || !matches!(&pending.phase,ForwardPhase::AwaitingHolder {installation} if installation==&source) {return Ok(());}
                    scope.finish_forward(pending,ForwardOutcome::Answered {reply:request.reply}).await
                })).await.map_err(|_|CallFailure::AnswerLost)?;
                // Unknown, late and wrong-holder answers are harmless acknowledgements.
                serde_json::to_vec(&()).map_err(|_| CallFailure::AnswerLost)
            }
            _ => Err(unsent()),
        }
    }
}
fn unsent() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
