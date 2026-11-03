//! Exact production forwarding boundaries reuse the original real socket oracle.
#![deny(clippy::too_many_lines)]
use super::forwarded_post_lifecycle_oracles::{answer, holder_frame, outcome, post};
use super::forwarded_post_wire::{next, next_with_bound};
use board_sync_protocol::{BoardConsumerAnswer, ForwardOutcome};
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeFaultObservation,
};
use collaboration_central_core::RegistryTime;
use fleet_sync_protocol::{DatabaseId, OperationAnswer, ServerFrame};
use reqwest_websocket::WebSocket;
use std::{future::Future, time::Duration};

pub(super) async fn exact_boundary<Control, ControlFuture>(
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
    let mut outstanding = Vec::new();
    for _ in 0..16 {
        outstanding.push((post(sender, database).await?, holder_frame(holder).await?));
    }
    control(RuntimeControl::SetTime {
        now: RegistryTime(started + chrono::TimeDelta::milliseconds(29_999)),
    })
    .await?;
    let overflow = post(sender, database).await?;
    if outcome(sender, overflow).await? != ForwardOutcome::Overloaded {
        return Err(
            "a pending forward released its slot before the literal 30-second boundary".into(),
        );
    }
    // A reply at 29.999s independently catches an early deadline even if its alarm has not run yet.
    let (before_boundary, before_holder_frame) =
        outstanding.pop().ok_or("sixteenth slot absent")?;
    let reply = answer(holder, before_holder_frame).await?;
    if outcome(sender, before_boundary).await? != (ForwardOutcome::Answered { reply }) {
        return Err("a holder answer at 29.999 seconds did not settle its still-owned slot".into());
    }
    let replacement = post(sender, database).await?;
    let fresh_holder_frame = holder_frame(holder).await?;
    if outstanding
        .iter()
        .any(|(_, frame)| *frame == fresh_holder_frame)
        || fresh_holder_frame == before_holder_frame
    {
        return Err("fresh forward reused an outstanding holder correlation identity".into());
    }
    control(RuntimeControl::SetTime {
        now: RegistryTime(started + chrono::TimeDelta::seconds(30)),
    })
    .await?;
    let old_holder_frame = outstanding
        .first()
        .ok_or("original pending group absent")?
        .1;
    let mut expected = outstanding
        .iter()
        .map(|(frame, _)| *frame)
        .collect::<std::collections::BTreeSet<_>>();
    for _ in 0..15 {
        let ServerFrame::Answer {
            frame,
            body:
                OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(
                    ForwardOutcome::Unanswered,
                )),
        } = next(sender).await?
        else {
            return Err("an original forward did not expire exactly once at 30 seconds".into());
        };
        if !expected.remove(&frame) {
            return Err(
                "the 30-second group duplicated or changed an original request identity".into(),
            );
        }
    }
    if !expected.is_empty() {
        return Err("the original deadline left an unanswered correlation pending".into());
    }
    let _late = answer(holder, old_holder_frame).await?;
    let reply = answer(holder, fresh_holder_frame).await?;
    if outcome(sender, replacement).await? != (ForwardOutcome::Answered { reply }) {
        return Err("late reply changed the fresh forward's exact holder answer".into());
    }
    Ok(())
}

pub(super) async fn alarm_event<Control, ControlFuture>(
    base: &str,
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
    let original = post(sender, database).await?;
    let old_holder_frame = holder_frame(holder).await?;
    let expected_deadline =
        collaboration_central_core::UnixMillis(started.timestamp_millis() + 30_000);
    let armed = super::forwarded_post_state_cases::wake_deadline(base).await?;
    if armed != Some(expected_deadline) {
        return Err(format!(
            "forward alarm was not armed at the literal 30000ms deadline before clock advance: expected {expected_deadline:?}, observed {armed:?}"
        ).into());
    }
    println!(
        "FORWARD_ALARM_ARMED literal_deadline_millis={}",
        expected_deadline.0
    );
    // Advancing the owning clock wakes the actual platform alarm. The socket
    // answer remains the oracle; the unchanged bound only contains failure.
    control(RuntimeControl::SetTime {
        now: RegistryTime(started + chrono::TimeDelta::seconds(30)),
    })
    .await?;
    let frame = next_with_bound(sender, Duration::from_secs(35)).await?;
    if !matches!(frame, ServerFrame::Answer {
        frame,
        body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(ForwardOutcome::Unanswered)),
    } if frame == original)
    {
        return Err(
            "the actual production alarm did not return the correlated Unanswered outcome".into(),
        );
    }
    let mut replacement = None;
    // Filling all sixteen slots proves the expired request no longer consumes one.
    for _ in 0..16 {
        let request = post(sender, database).await?;
        let delivered = holder_frame(holder).await?;
        if delivered == old_holder_frame {
            return Err(
                "the controlled alarm allowed a stale holder correlation to be reused".into(),
            );
        }
        replacement = Some((request, delivered));
    }
    let overflow = post(sender, database).await?;
    if outcome(sender, overflow).await? != ForwardOutcome::Overloaded {
        return Err("post-alarm capacity did not retain exactly sixteen fresh slots".into());
    }
    let _late = answer(holder, old_holder_frame).await?;
    let (request, delivered) = replacement.ok_or("post-alarm replacement missing")?;
    let reply = answer(holder, delivered).await?;
    if outcome(sender, request).await? != (ForwardOutcome::Answered { reply }) {
        return Err("the controlled alarm did not preserve independent fresh correlation".into());
    }
    Ok(())
}
