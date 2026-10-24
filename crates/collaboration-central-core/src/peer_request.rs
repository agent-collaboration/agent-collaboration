//! Select a live binding under exclusion, then send without holding that section.

use crate::{
    ObjectHost,
    holder_reply_hub::{HolderReplyHub, ReplyWaiter},
    object_call_envelope::encode_refusal,
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::CallFailure;
use fleet_sync_protocol::{FrameId, PeerId, ServerCall, ServerFrame, ServerRequest};
use fleet_sync_session::{ConnectionRegistration, SessionKey};
use runtime_primitives::{Clock, IdSource, WakeScheduler};
use std::{sync::Arc, time::Duration};

pub(crate) async fn begin_peer_request<Host, Time>(
    host: Arc<Host>,
    time: Arc<Time>,
    replies: Arc<HolderReplyHub>,
    peer: PeerId,
    body: ServerCall<BoardDeliveryTypes>,
    timeout: Duration,
) -> Result<ReplyWaiter, CallFailure>
where
    Host: ObjectHost,
    Time: Clock + IdSource + Send + Sync + 'static,
{
    let frame = FrameId::new(time.new_id()).map_err(encode_refusal)?;
    let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: ServerRequest::Crate(body),
    })
    .map_err(encode_refusal)?;
    let selection_host = host.clone();
    let (connection, waiter, deadline) = host
        .with_local_section(async move {
            for (connection, registration) in
                selection_host.registrations().await.map_err(|_| unsent())?
            {
                let ConnectionRegistration::Attached { session_id, .. } = registration else {
                    continue;
                };
                let Ok(binding) = selection_host.admission_binding(connection).await else {
                    continue;
                };
                let now = time.now();
                if binding.connection != connection
                    || binding.peer != peer
                    || binding.expires_at <= now
                {
                    continue;
                }
                let deadline = now.after(timeout);
                let waiter = replies.register(
                    SessionKey { peer, session_id },
                    connection,
                    frame,
                    deadline,
                )?;
                return Ok((connection, waiter, deadline));
            }
            Err(unsent())
        })
        .await
        .map_err(|_| unsent())??;
    host.wake().wake_at(deadline).await.map_err(|_| unsent())?;
    host.send_frame(connection, bytes.into())
        .await
        .map_err(|_| CallFailure::AnswerLost)?;
    Ok(waiter)
}

fn unsent() -> CallFailure {
    CallFailure::NotSent {
        reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
    }
}
