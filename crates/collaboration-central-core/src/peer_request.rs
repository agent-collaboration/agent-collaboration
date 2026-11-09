//! Select a live binding under exclusion, then send without holding that section.

use crate::{
    ObjectHost,
    holder_reply_hub::{HolderReplyHub, ReplyWaiter},
    object_call_envelope::encode_refusal,
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{
    AtomicStore, CallFailure, CallRetryReason, StoreError, storage::TransactionScope,
};
use fleet_protocol::{FrameId, PeerId, ServerCall, ServerFrame, ServerRequest};
use fleet_session::{ConnectionRegistration, SessionKey};
use futures_util::future::{Either, select};
use runtime_primitives::{Clock, IdSource, WakeScheduler};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{future::Future, sync::Arc, time::Duration};

pub(crate) async fn begin_peer_request<Host, Time, MakeDeadline, Deadline>(
    host: Arc<Host>,
    time: Arc<Time>,
    replies: Arc<HolderReplyHub>,
    peer: PeerId,
    body: ServerCall<BoardDeliveryTypes>,
    timeout: Duration,
    deadline_after: MakeDeadline,
) -> Result<ReplyWaiter, CallFailure>
where
    Host: ObjectHost,
    for<'t> <Host::Store as AtomicStore>::Scope<'t>: TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'v> String: Type<Host::Database> + Encode<'v, Host::Database> + Decode<'v, Host::Database>,
    for<'v> Option<String>:
        Type<Host::Database> + Encode<'v, Host::Database> + Decode<'v, Host::Database>,
    for<'r> &'r str: ColumnIndex<<Host::Database as Database>::Row>,
    Time: Clock + IdSource + Send + Sync + 'static,
    MakeDeadline: FnOnce(Duration) -> Deadline + Send,
    Deadline: Future<Output = ()> + Send,
{
    let frame = FrameId::new(time.new_id()).map_err(encode_refusal)?;
    let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: ServerRequest::Crate(body),
    })
    .map_err(encode_refusal)?;
    let selection_host = host.clone();
    let selection_time = time.clone();
    let (connection, waiter, deadline) = host
        .with_local_section(async move {
            let selection_store = selection_host.store().clone();
            selection_store
                .transaction(move |scope| {
                    Box::pin(async move {
                        let now = selection_time.now();
                        if crate::machine_enrollment::admission::read_current_enrolled_machine(
                            scope, peer, now,
                        )
                        .await?
                        .is_none()
                        {
                            return Ok(Err(unsent()));
                        }
                        let Ok(registrations) = selection_host.registrations().await else {
                            return Ok(Err(unsent()));
                        };
                        for (connection, registration) in registrations {
                            let ConnectionRegistration::Attached { session_id, .. } = registration
                            else {
                                continue;
                            };
                            let Ok(binding) = selection_host.admission_binding(connection).await
                            else {
                                continue;
                            };
                            if binding.connection != connection
                                || binding.peer != peer
                                || binding.expires_at <= now
                            {
                                continue;
                            }
                            let deadline = now.after(timeout);
                            // The particular waiter is selected before this enrollment
                            // transaction commits. Rollback drops its RAII correlation.
                            return Ok(replies
                                .register(
                                    SessionKey { peer, session_id },
                                    connection,
                                    frame,
                                    deadline,
                                )
                                .map(|waiter| (connection, waiter, deadline)));
                        }
                        Ok(Err(unsent()))
                    })
                })
                .await
                .map_err(|_: StoreError| CallFailure::AnswerLost)?
        })
        .await
        .map_err(|_| unsent())??;
    let remaining = Duration::from_millis(
        u64::try_from(
            deadline
                .unix_millis()
                .saturating_sub(time.now().unix_millis()),
        )
        .unwrap_or(0),
    );
    let send = async {
        host.wake().wake_at(deadline).await.map_err(|_| unsent())?;
        host.send_frame(connection, bytes.into())
            .await
            .map_err(|_| CallFailure::AnswerLost)
    };
    match select(Box::pin(send), Box::pin(deadline_after(remaining))).await {
        Either::Left((sent, _)) => sent?,
        Either::Right(_) => {
            return Err(CallFailure::Retryable {
                reason: CallRetryReason {
                    code: "peerSendDeadline".into(),
                    detail: "peer request could not enqueue before its answer deadline".into(),
                },
            });
        }
    }
    Ok(waiter)
}

fn unsent() -> CallFailure {
    CallFailure::NotSent {
        reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
    }
}

#[cfg(test)]
#[path = "peer_selection_test.rs"]
mod enrollment_tests;
