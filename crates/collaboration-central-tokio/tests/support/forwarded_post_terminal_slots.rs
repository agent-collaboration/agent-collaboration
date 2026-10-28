//! Each terminal cause releases its own slot while fifteen independent forwards remain live.
#![deny(clippy::too_many_lines)]
use super::forwarded_post_lifecycle_oracles::{answer, holder_frame, outcome, post};
use board_sync_protocol::ForwardOutcome;
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeFaultObservation,
};
use collaboration_central_core::RegistryTime;
use fleet_sync_protocol::{DatabaseId, FrameId};
use reqwest_websocket::WebSocket;
use std::{
    future::Future,
    time::{Duration, SystemTime},
};

type Outstanding = Vec<(FrameId, FrameId)>;

async fn reserve(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    count: usize,
) -> ConformanceResult<Outstanding> {
    let mut outstanding = Vec::new();
    for _ in 0..count {
        outstanding.push((post(sender, database).await?, holder_frame(holder).await?));
    }
    Ok(outstanding)
}

async fn settle(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    request: FrameId,
    delivered: FrameId,
) -> ConformanceResult<()> {
    let reply = answer(holder, delivered).await?;
    if outcome(sender, request).await? != (ForwardOutcome::Answered { reply }) {
        return Err("terminal release changed a still-live holder reply identity".into());
    }
    Ok(())
}

async fn replacement_at_capacity(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<(FrameId, FrameId)> {
    let replacement = post(sender, database).await?;
    let delivered = holder_frame(holder).await?;
    let overflow = post(sender, database).await?;
    if outcome(sender, overflow).await? != ForwardOutcome::Overloaded {
        return Err("a terminal cause released another request's retained slot".into());
    }
    Ok((replacement, delivered))
}

pub(super) async fn terminal_release<Control, ControlFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    sender_owned: DatabaseId,
    mut control: Control,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
{
    let started = SystemTime::now();
    control(RuntimeControl::SetTime {
        now: RegistryTime(started.into()),
    })
    .await?;
    let outstanding = reserve(sender, holder, database, 15).await?;
    for (destination, expected) in [
        (sender_owned, ForwardOutcome::SenderHolds),
        (
            DatabaseId::new(durable_steps::Uuid::now_v7())?,
            ForwardOutcome::NotHeld,
        ),
    ] {
        let request = post(sender, destination).await?;
        if outcome(sender, request).await? != expected {
            return Err("routing terminal returned another cause's outcome".into());
        }
        let (replacement, delivered) = replacement_at_capacity(sender, holder, database).await?;
        settle(sender, holder, replacement, delivered).await?;
    }
    let answered = post(sender, database).await?;
    let delivered = holder_frame(holder).await?;
    settle(sender, holder, answered, delivered).await?;
    let (replacement, delivered) = replacement_at_capacity(sender, holder, database).await?;
    settle(sender, holder, replacement, delivered).await?;
    for (request, delivered) in outstanding {
        settle(sender, holder, request, delivered).await?;
    }
    deadline_release(sender, holder, database, started, &mut control).await
}

async fn deadline_release<Control, ControlFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    started: SystemTime,
    control: &mut Control,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
{
    let old = post(sender, database).await?;
    let old_delivered = holder_frame(holder).await?;
    control(RuntimeControl::SetTime {
        now: RegistryTime((started + Duration::from_secs(1)).into()),
    })
    .await?;
    let younger = reserve(sender, holder, database, 15).await?;
    let overflow = post(sender, database).await?;
    if outcome(sender, overflow).await? != ForwardOutcome::Overloaded {
        return Err("deadline setup failed to retain exactly sixteen original slots".into());
    }
    control(RuntimeControl::SetTime {
        now: RegistryTime((started + Duration::from_secs(30)).into()),
    })
    .await?;
    if outcome(sender, old).await? != ForwardOutcome::Unanswered {
        return Err("older deadline did not return its exact Unanswered outcome".into());
    }
    let (replacement, delivered) = replacement_at_capacity(sender, holder, database).await?;
    if delivered == old_delivered || younger.iter().any(|(_, frame)| *frame == delivered) {
        return Err("deadline replacement reused a prior holder frame".into());
    }
    let _late = answer(holder, old_delivered).await?;
    settle(sender, holder, replacement, delivered).await?;
    for (request, delivered) in younger {
        settle(sender, holder, request, delivered).await?;
    }
    Ok(())
}
