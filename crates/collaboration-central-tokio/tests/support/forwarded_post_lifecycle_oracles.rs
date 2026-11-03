//! Original Native forwarding lifecycle oracles on real socket/control boundaries.
use super::forwarded_post_wire::{next, send, send_forward};
use board_sync_protocol::{
    BoardClientAnswer, BoardConsumerAnswer, BoardPayload, BoardServerRequest, ForwardOutcome,
    ForwardedPost, ForwardedPostKind,
};
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeFaultObservation,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DatabaseId, FrameId, OperationAnswer, ServerFrame, ServerRequest,
};
use message_board::ProjectId;
use reqwest_websocket::WebSocket;
use std::future::Future;

pub(super) const POST_MARKER: &str = "forward-body-must-stay-transient-476";

pub(super) async fn post(
    sender: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<FrameId> {
    send_forward(
        sender,
        ProjectId::try_from(database.to_string())?,
        ForwardedPost {
            kind: ForwardedPostKind::ThreadPost,
            body: BoardPayload::encode(&serde_json::json!({"opaque":POST_MARKER}))?,
        },
    )
    .await
}
pub(super) async fn holder_frame(holder: &mut WebSocket) -> ConformanceResult<FrameId> {
    let ServerFrame::Request {
        frame,
        body: ServerRequest::Consumer(BoardServerRequest::ForwardedPost(_)),
    } = next(holder).await?
    else {
        return Err("holder did not receive the forwarded request".into());
    };
    Ok(frame)
}
pub(super) async fn answer(
    holder: &mut WebSocket,
    frame: FrameId,
) -> ConformanceResult<BoardPayload> {
    let reply = BoardPayload::encode(&serde_json::json!({"opaque":"transient-holder-decision"}))?;
    send(
        holder,
        ClientFrame::Answer {
            frame,
            body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(reply.clone())),
        },
    )
    .await?;
    Ok(reply)
}
pub(super) async fn outcome(
    sender: &mut WebSocket,
    expected: FrameId,
) -> ConformanceResult<ForwardOutcome> {
    let ServerFrame::Answer {
        frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
    } = next(sender).await?
    else {
        return Err("forward did not return its typed terminal outcome".into());
    };
    if frame != expected {
        return Err("forward terminal response lost its exact frame correlation".into());
    }
    Ok(outcome)
}

pub(super) async fn capacity<Control, ControlFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    mut control: Control,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
{
    let mut outstanding = Vec::new();
    for _ in 0..15 {
        outstanding.push((post(sender, database).await?, holder_frame(holder).await?));
    }
    control(RuntimeControl::HoldNextDispatch {
        destination: ObjectAddress::Registry,
    })
    .await?;
    let sixteenth = post(sender, database).await?;
    control(RuntimeControl::AwaitHeldDispatch).await?;
    let seventeenth = post(sender, database).await?;
    let observed = outcome(sender, seventeenth).await;
    if observed.is_err() {
        // A wrong build must still release the exact barrier this oracle owns.
        let _ = control(RuntimeControl::ResumeDispatch).await;
    }
    let refused = observed.map_err(|error| {
        format!("seventeenth forward did not answer Overloaded before Registry dispatch: {error}")
    })?;
    if refused != ForwardOutcome::Overloaded {
        let _ = control(RuntimeControl::ResumeDispatch).await;
        return Err("seventeenth forward lost its exact Overloaded outcome".into());
    }
    control(RuntimeControl::ResumeDispatch).await?;
    let delivered = holder_frame(holder).await?;
    let reply = answer(holder, delivered).await?;
    if outcome(sender, sixteenth).await? != (ForwardOutcome::Answered { reply }) {
        return Err("sixteenth forward did not retain its exact holder reply".into());
    }
    let replacement = post(sender, database).await?;
    let delivered = holder_frame(holder).await?;
    let reply = answer(holder, delivered).await?;
    if outcome(sender, replacement).await? != (ForwardOutcome::Answered { reply }) {
        return Err("terminal forward did not free its actual reserved capacity slot".into());
    }
    if outstanding.len() != 15 {
        return Err("capacity setup lost an original outstanding identity".into());
    }
    Ok(())
}

pub(super) async fn deadline<Control, ControlFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    mut control: Control,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
{
    let started = collaboration_central_conformance::fixture_time()?.0;
    control(RuntimeControl::SetTime {
        now: RegistryTime(started),
    })
    .await?;
    let original = post(sender, database).await?;
    let old_holder_frame = holder_frame(holder).await?;
    control(RuntimeControl::SetTime {
        now: RegistryTime(started + chrono::TimeDelta::seconds(30)),
    })
    .await?;
    if outcome(sender, original).await? != ForwardOutcome::Unanswered {
        return Err("original forward deadline lost its exact Unanswered outcome".into());
    }
    let _late = answer(holder, old_holder_frame).await?;
    let replacement = post(sender, database).await?;
    let fresh_holder_frame = holder_frame(holder).await?;
    if fresh_holder_frame == old_holder_frame {
        return Err("fresh forward reused the expired holder correlation identity".into());
    }
    let reply = answer(holder, fresh_holder_frame).await?;
    if outcome(sender, replacement).await? != (ForwardOutcome::Answered { reply }) {
        return Err("late reply changed the fresh forward's exact holder answer".into());
    }
    Ok(())
}
