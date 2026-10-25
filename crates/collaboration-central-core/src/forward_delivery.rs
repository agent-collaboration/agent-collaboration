//! Delivery accepts verified object callers; opaque replies travel only after commit.
use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
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
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(unsent());
        }
        let ObjectAddress::Connection(source) = invocation.source else {
            return Err(unsent());
        };
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            installation,
            ..
        } = &self.work
        else {
            return Err(unsent());
        };
        let store = CoreSessionStore::new(host.clone(), shared_host.clone(), replies.clone());
        match invocation.kind.as_str() {
            DeliverForward::KIND => {
                let target: DeliverForward =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                if target.installation != *installation {
                    return Err(unsent());
                }
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
                                {
                                    return Err(StoreError::Corrupt {
                                        field: "delivered_forwards.forward_frame".into(),
                                        message: "correlation identity was reused".into(),
                                    });
                                }
                                return Ok(ForwardDeliveryResult::Delivered);
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
                let target: AnswerForward =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                if target.installation != *installation {
                    return Err(unsent());
                }
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
