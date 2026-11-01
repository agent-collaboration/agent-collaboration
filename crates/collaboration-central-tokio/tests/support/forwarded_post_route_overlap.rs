//! Hold the original real route beyond expiry and observe its completed late callback.
#![deny(clippy::too_many_lines)]
use super::{
    forwarded_post_lifecycle_oracles::{answer, holder_frame, outcome, post},
    forwarded_post_scenarios::SENDER_SERVICE,
    forwarded_post_state_oracles::{
        live_records, no_retained_state, pending_record, records, sender_session,
    },
};
use board_sync_protocol::ForwardOutcome;
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchFaultObservation, RuntimeControl,
    RuntimeFaultObservation,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_sync_protocol::{DatabaseId, FrameId};
use reqwest_websocket::WebSocket;
use serde_json::{Value, json};
use std::{future::Future, time::Duration};

pub(super) async fn late_route<Control, ControlFuture, Observe, ObserveFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    mut control: Control,
    mut observe: Observe,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
    Observe: FnMut(bool) -> ObserveFuture,
    ObserveFuture: Future<Output = ConformanceResult<Value>>,
{
    let started = collaboration_central_conformance::fixture_time()?.0;
    control(RuntimeControl::SetTime {
        now: RegistryTime(started),
    })
    .await?;
    control(RuntimeControl::HoldNextSelectedDispatch {
        destination: ObjectAddress::Registry,
        call: DispatchCallSelector::ForwardRoute,
    })
    .await?;
    let proof = overlap_sequence(
        sender,
        holder,
        database,
        started,
        &mut control,
        &mut observe,
    )
    .await;
    let released = match control(RuntimeControl::Observe).await {
        Ok(state) if matches!(state.dispatch, DispatchFaultObservation::Held { .. }) => {
            control(RuntimeControl::ResumeDispatch).await.map(|_| ())
        }
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    };
    match (proof, released) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(cleanup)) => {
            Err(format!("route overlap: {proof}; barrier cleanup: {cleanup}").into())
        }
    }
}

async fn overlap_sequence<Control, ControlFuture, Observe, ObserveFuture>(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    database: DatabaseId,
    started: chrono::DateTime<chrono::Utc>,
    control: &mut Control,
    observe: &mut Observe,
) -> ConformanceResult<()>
where
    Control: FnMut(RuntimeControl) -> ControlFuture,
    ControlFuture: Future<Output = ConformanceResult<RuntimeFaultObservation>>,
    Observe: FnMut(bool) -> ObserveFuture,
    ObserveFuture: Future<Output = ConformanceResult<Value>>,
{
    let original = post(sender, database).await?;
    require_route_hold(control(RuntimeControl::AwaitHeldDispatch).await?, database)?;
    let pending = pending_record(
        &observe(false).await?,
        database,
        original,
        json!({"kind":"routing"}),
    )?;
    let old_frame: FrameId = serde_json::from_value(
        pending
            .get("forwardFrame")
            .ok_or("forward identity absent")?
            .clone(),
    )?;
    control(RuntimeControl::SetTime {
        now: RegistryTime(started + chrono::TimeDelta::seconds(30)),
    })
    .await?;
    if outcome(sender, original).await? != ForwardOutcome::Unanswered {
        return Err("held original route did not expire at its original deadline".into());
    }
    let fresh = post(sender, database).await?;
    let fresh_delivered = holder_frame(holder).await?;
    if fresh_delivered == old_frame {
        return Err("fresh request reused the expired route frame".into());
    }
    control(RuntimeControl::ResumeDispatch).await?;
    await_completed(observe, old_frame).await?;
    live_records(
        &observe(false).await?,
        &observe(true).await?,
        database,
        fresh,
        fresh_delivered,
    )?;
    // An answer to the expired identity cannot settle the live replacement.
    let _late = answer(holder, old_frame).await?;
    let reply = answer(holder, fresh_delivered).await?;
    if outcome(sender, fresh).await? != (ForwardOutcome::Answered { reply }) {
        return Err("late routing/answer duplicated a terminal or replaced the fresh reply".into());
    }
    no_retained_state(&observe(false).await?, &observe(true).await?)
}

fn require_route_hold(
    held: RuntimeFaultObservation,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let DispatchFaultObservation::Held {
        destination,
        request,
    } = held.dispatch
    else {
        return Err("the original forwarding route never reached its real hold".into());
    };
    let envelope: Value = serde_json::from_slice(&request)?;
    for (field, expected) in [
        ("kind", json!("central.forward-route")),
        (
            "source",
            json!({"kind":"connection","value":SENDER_SERVICE}),
        ),
        ("sourceFace", json!("session")),
        ("destination", json!({"kind":"registry"})),
        ("destinationFace", json!("registry")),
        (
            "request",
            json!({"session":sender_session(),"project":database}),
        ),
    ] {
        if envelope.get(field) != Some(&expected) {
            return Err(format!("held forwarding call changed its {field}").into());
        }
    }
    let identity: durable_steps::CallId = serde_json::from_value(
        envelope
            .get("identity")
            .ok_or("held forwarding CallId absent")?
            .clone(),
    )?;
    println!(
        "FORWARD_ROUTE_HOLD call={}",
        serde_json::to_string(&identity)?
    );
    if destination != ObjectAddress::Registry {
        return Err("held forwarding call lost destination or CallId".into());
    }
    Ok(())
}

async fn await_completed<Observe, ObserveFuture>(
    observe: &mut Observe,
    frame: FrameId,
) -> ConformanceResult<()>
where
    Observe: FnMut(bool) -> ObserveFuture,
    ObserveFuture: Future<Output = ConformanceResult<Value>>,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let state = observe(false).await?;
            if records(&state, "completedRoutes")?.contains(&json!(frame)) {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
