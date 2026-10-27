//! Retained forwarding assertions shared by actual Native and Worker sessions.

use super::forwarded_post_wire::{next, send, send_forward};
use board_sync_protocol::{
    BoardClientAnswer, BoardConsumerAnswer, BoardPayload, BoardServerRequest, ForwardOutcome,
    ForwardedPost, ForwardedPostKind,
};
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DatabaseId, FrameId, OperationAnswer, ServerFrame, ServerRequest,
};
use message_board::{ActorOrigin, ProjectId};
use reqwest_websocket::WebSocket;

pub(super) async fn preserves_origin_and_opaque_reply(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    expected_origin: ActorOrigin,
) -> ConformanceResult<()> {
    let project = ProjectId::try_from(database.to_string())?;
    let post = ForwardedPost {
        kind: ForwardedPostKind::ThreadPost,
        body: BoardPayload::encode(&serde_json::json!({
            "secretPostMarker": "never-persist-this",
            "exactInteger": i64::MAX
        }))?,
    };
    let sender_frame = send_forward(sender, project.clone(), post.clone()).await?;
    let ServerFrame::Request {
        frame: holder_frame,
        body: ServerRequest::Consumer(BoardServerRequest::ForwardedPost(received)),
    } = next(holder).await?
    else {
        return Err("holder did not receive its typed forwarded request".into());
    };
    if received.project != project || received.post != post {
        return Err("forward changed its exact project or opaque request body".into());
    }
    if received.origin != expected_origin {
        return Err(format!(
            "forward changed its verified origin: {:?}; expected {expected_origin:?}",
            received.origin
        )
        .into());
    }
    let reply = BoardPayload::encode(&serde_json::json!({
        "opaqueReply": [true, null, i64::MAX]
    }))?;
    send(
        holder,
        ClientFrame::Answer {
            frame: holder_frame,
            body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(reply.clone())),
        },
    )
    .await?;
    let ServerFrame::Answer {
        frame: answered_frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
    } = next(sender).await?
    else {
        return Err("sender did not receive its typed forwarding answer".into());
    };
    if answered_frame != sender_frame || outcome != (ForwardOutcome::Answered { reply }) {
        return Err("forward changed its exact sender correlation or opaque holder reply".into());
    }
    Ok(())
}

pub(super) async fn disconnected_holder_has_registry_presence(
    sender: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let sender_frame = send_forward(
        sender,
        ProjectId::try_from(database.to_string())?,
        ForwardedPost {
            kind: ForwardedPostKind::ThreadCreate,
            body: BoardPayload::encode(&serde_json::json!({"body":"opaque"}))?,
        },
    )
    .await?;
    let ServerFrame::Answer {
        frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
    } = next(sender).await?
    else {
        return Err("disconnected holder closed the sender instead of answering".into());
    };
    if frame != sender_frame
        || !matches!(
            outcome,
            ForwardOutcome::HolderUnreachable { last_seen: Some(_) }
        )
    {
        return Err(format!("disconnected holder lost its exact correlation or stored last-seen presence: {outcome:?}").into());
    }
    Ok(())
}

pub(super) struct OutstandingForward {
    sender_frame: FrameId,
    holder_frame: FrameId,
}
pub(super) async fn begin_reconnect(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<OutstandingForward> {
    let sender_frame = send_forward(
        sender,
        ProjectId::try_from(database.to_string())?,
        ForwardedPost {
            kind: ForwardedPostKind::ThreadPost,
            body: BoardPayload::encode(&serde_json::json!({"reconnect":"opaque post"}))?,
        },
    )
    .await?;
    let ServerFrame::Request {
        frame: holder_frame,
        body: ServerRequest::Consumer(BoardServerRequest::ForwardedPost(_)),
    } = next(holder).await?
    else {
        return Err("holder did not receive the original forwarded post".into());
    };
    Ok(OutstandingForward {
        sender_frame,
        holder_frame,
    })
}
pub(super) async fn finish_reconnect(
    sender: &mut WebSocket,
    replacement: &mut WebSocket,
    outstanding: OutstandingForward,
) -> ConformanceResult<()> {
    let reply = BoardPayload::encode(&serde_json::json!({"reconnected":"recorded decision"}))?;
    send(
        replacement,
        ClientFrame::Answer {
            frame: outstanding.holder_frame,
            body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(reply.clone())),
        },
    )
    .await?;
    let ServerFrame::Answer {
        frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
    } = next(sender).await.map_err(|error| {
        format!("reconnected holder did not answer its original correlation: {error}")
    })?
    else {
        return Err("reconnected holder lost the original request correlation".into());
    };
    if frame != outstanding.sender_frame || outcome != (ForwardOutcome::Answered { reply }) {
        return Err(
            "reconnected holder lost the exact original correlation or opaque reply".into(),
        );
    }
    Ok(())
}
