//! Delivery accepts verified object callers; opaque replies travel only after commit.
use crate::{
    CallerKind, DeliveryKind, LegacyObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    forward_calls::{DeliverForward, ForwardDeliveryRequest, ForwardDeliveryResult},
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    project_request_store::{self, ProjectRequestPhase},
    session_store::{CoreSessionStore, SessionTransportEffect},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardServerRequest};
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

impl<Host: LegacyObjectHost, Io: PlatformIo> ObjectRuntime<DeliveryKind, Host, Io>
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
                            let Some(mut existing) =
                                project_request_store::by_frame(scope, request.forward_frame)
                                    .await?
                            else {
                                return Ok(ForwardDeliveryResult::NoConnection);
                            };
                            let mut requester_rows = crate::generated_enrollment::read_enrollments(
                                scope,
                                None,
                                None,
                                None,
                                Some(existing.session.peer.to_string()),
                            )
                            .await
                            .map_err(StoreError::backend)?;
                            if requester_rows.len() > 1 {
                                return Err(StoreError::Corrupt {
                                    field: "machine_enrollments.peer_id".into(),
                                    message: "duplicate requester mapping".into(),
                                });
                            }
                            let requester_service = requester_rows
                                .pop()
                                .map(|row| {
                                    crate::machine_enrollment_record::service_for_stored_peer(
                                        row,
                                        existing.session.peer,
                                    )
                                })
                                .transpose()?;
                            if existing.project != request.post.project
                                || existing.deadline != request.deadline
                                || existing.session != request.origin_session
                                || existing.request_frame != request.request_frame
                                || existing.connection != request.connection
                                || existing.created_at != request.created_at
                                || requester_service.as_ref() != Some(&source)
                            {
                                return Err(StoreError::Corrupt {
                                    field: "machine_project_requests.id".into(),
                                    message: "request identity was reused".into(),
                                });
                            }
                            match existing.phase {
                                ProjectRequestPhase::AtHolder { holder }
                                    if holder == request.holder =>
                                {
                                    return Ok(ForwardDeliveryResult::Delivered);
                                }
                                ProjectRequestPhase::AwaitingHolder { holder }
                                    if holder == request.holder => {}
                                _ => {
                                    return Err(StoreError::Corrupt {
                                        field: "machine_project_requests.phase".into(),
                                        message: "delivery differs from the selected holder".into(),
                                    });
                                }
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
                                existing.phase = ProjectRequestPhase::AtHolder {
                                    holder: binding.peer,
                                };
                                existing.updated_at = now;
                                project_request_store::save(scope, &existing).await?;
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
            _ => Err(unsent()),
        }
    }
}
fn unsent() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
