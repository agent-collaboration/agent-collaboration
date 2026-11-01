//! A live Engine drive keeps replay Pending until its full Copy attempt ends.

use super::{
    copy_wait_scenarios::{CopyClient, database, expected, position},
    holder_writer_retry::{owner_post, received, send},
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion,
};
use durable_steps::OperationId;
use fleet_sync_protocol::{
    AwaitAckedAnswer, Epoch, FrameId, ReleaseAnswer, ServerCall, ServerCallAnswer,
};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::task::JoinSet;

pub const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400d5";

pub async fn run<Observe, Observed>(base: &str, mut observe: Observe) -> ConformanceResult<()>
where
    Observe: FnMut() -> Observed,
    Observed: Future<Output = ConformanceResult<Vec<FrameId>>>,
{
    use collaboration_central_core::{ObjectAddress, RegistryTime};
    let clock = ConformanceClient::new(base)?;
    let started = collaboration_central_conformance::fixture_time()?;
    let copy = CopyClient::new(base)?;
    let database = database("911")?;
    let connection =
        ObjectAddress::Connection(message_board::ServiceId::try_from(SERVICE.to_owned())?);
    for address in [ObjectAddress::Registry, ObjectAddress::Copy(database)] {
        clock.set_object_time(address, started).await?;
    }
    copy.create(database).await?;
    let mut peer = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        "019f0000-0000-7000-8000-00000000101f",
        "copy-drive-overlap",
        "019f0000-0000-7000-8000-000000000912",
    )
    .await?;
    clock.set_object_time(connection.clone(), started).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(75))
        .build()?;
    let input = json!({
        "attempt":"019f0000-0000-7000-8000-000000000913",
        "copy":"019f0000-0000-7000-8000-000000000010",
        "request":{"kind":"releaseThenAcknowledged","value":{
            "holder":peer.peer,
            "request":{"database":database,"epoch":1,
                "transfer":"019f0000-0000-7000-8000-000000000914"}
        }}
    });
    let mut callers = JoinSet::new();
    start(&http, base, &mut callers, input.clone());
    let result = async {
        let (frame, first) = received(&mut peer.socket).await?;
        let ServerCall::Release { call, request } = first else {
            return Err("sequential caller did not send Release".into());
        };
        if request.database != database || observe().await? != vec![frame] {
            return Err("sequential Release did not own exactly its expected waiter".into());
        }
        let release_at = RegistryTime(started.0 + chrono::TimeDelta::seconds(18));
        for address in [ObjectAddress::Registry, ObjectAddress::Copy(database), connection.clone()] {
            clock.set_object_time(address, release_at).await?;
        }
        send(
            &mut peer.socket,
            frame,
            ServerCallAnswer::Release(ReleaseAnswer::Released {
                final_position: position(1)?,
            }),
        )
        .await?;
        let first = copy
            .observed(database, |state| state.attempts == 1 && state.waiting == 1)
            .await?;
        let armed = first.deadlines.first().ok_or("Copy wait has no actual armed deadline")?;
        if armed.armed_at.0 != release_at.0.timestamp_millis()
            || armed.deadline.0 - armed.armed_at.0 != 20_000
        {
            return Err("the actual armed Copy deadline differs from the configured 20 seconds".into());
        }
        if !matches!(first.calls.as_slice(), [identity]
            if identity.operation == call.operation && identity.step.as_str() == "copy_request") {
            return Err("Copy wait did not follow Release in the same real operation".into());
        }
        let replay_at = RegistryTime(started.0 + chrono::TimeDelta::seconds(31));
        for address in [ObjectAddress::Registry, ObjectAddress::Copy(database), connection.clone()] {
            clock.set_object_time(address, replay_at).await?;
        }
        let before = copy.control(database, "observe").await?;
        if before.attempts != 1 || before.waiting != 1 || before.calls != first.calls {
            return Err("the original drive lost its single Copy waiter before replay".into());
        }
        // W-C44's live per-operation guard wins before the durable lease claim.
        // Await this exact replay response while the first Copy call is still
        // held, rather than assuming another HTTP event has already completed.
        let replay = tokio::time::timeout(
            Duration::from_secs(2),
            owner_post(&http, base, "copy-call", input),
        )
        .await??;
        let replay_operation: OperationId = serde_json::from_value(
            replay.pointer("/value/operation").cloned().ok_or("replay omitted its operation")?,
        )?;
        if replay.get("kind").and_then(Value::as_str) != Some("pending")
            || replay_operation != call.operation
        {
            return Err(format!("live-drive replay did not stay Pending on its original operation: {replay}").into());
        }
        let after_replay = copy.control(database, "observe").await?;
        if after_replay.attempts != 1 || after_replay.waiting != 1
            || after_replay.calls != first.calls || !observe().await?.is_empty()
        {
            return Err("Pending replay dispatched another Copy call or repeated Release".into());
        }
        // Only expiry of the original full Copy attempt releases the live drive.
        // The next attempt must replay the saved Release and retain its CallId.
        let expires_at = RegistryTime(
            chrono::DateTime::from_timestamp_millis(armed.deadline.0).ok_or("Copy fixture deadline")?,
        );
        for address in [ObjectAddress::Registry, connection.clone(), ObjectAddress::Copy(database)] {
            clock.set_object_time(address, expires_at).await?;
        }
        // The HTTP response follows recording this attempt's outcome. Only
        // then wake the durable caller again at the same controlled instant.
        callers.join_next().await.ok_or("Copy caller task disappeared")???;
        clock.set_object_time(ObjectAddress::Registry, expires_at).await?;
        let retried = copy.observed(database, |state| state.attempts == 2 && state.waiting == 1).await?;
        let elapsed = armed.deadline.0 - started.0.timestamp_millis();
        let waited = armed.deadline.0 - armed.armed_at.0;
        if elapsed < 37_000 || waited < 19_900
        {
            return Err("the Copy attempt ended before its full configured wait".into());
        }
        if !matches!(retried.calls.as_slice(), [old, retry] if old == retry && first.calls.first() == Some(old))
            || !observe().await?.is_empty()
        {
            return Err("retry changed the durable Copy CallId or repeated Release".into());
        }
        let rejected = rejected_tokens(&http, base).await?;
        if rejected.contains(&call.operation) {
            return Err("a same-Engine replay replaced its still-owned drive token".into());
        }
        let pending = copy.status(call.operation).await?;
        if pending.get("kind").and_then(Value::as_str) != Some("pending") {
            return Err(format!("below-P retry fabricated a terminal result: {pending}").into());
        }
        copy.retire(database, Epoch::new(1)?).await?;
        copy.completed(
            copy.status(call.operation).await?,
            expected("acknowledged", AwaitAckedAnswer::Retired),
        )
        .await?;
        let completed = copy.control(database, "observe").await?;
        if completed.attempts != 2 || completed.waiting != 0 {
            return Err("converged operation retained an obsolete Copy waiter".into());
        }
        println!(
            "COPY_DRIVE_CASE release_18s_then_copy_20s_crosses_30s_lease replay=pending replay_copy_calls=1 release_calls=1 copy_calls=2 retry_waiters=1 obsolete_waiters=0 actual_old_token_rejections={} elapsed_ms={elapsed} terminal=retired passed",
            rejected.iter().filter(|id| **id == call.operation).count()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    if let Err(error) = &result {
        eprintln!("COPY_DRIVE_SCENARIO failed: {error}");
        // Requests still awaiting a change must end before native shutdown
        // cancels the wake owner that would otherwise expire them.
        let _ = copy.retire(database, Epoch::new(1)?).await;
    }
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}

fn start(
    http: &reqwest::Client,
    base: &str,
    callers: &mut JoinSet<ConformanceResult<Value>>,
    input: Value,
) {
    let http = http.clone();
    let base = base.to_owned();
    callers.spawn(async move { owner_post(&http, &base, "copy-call", input).await });
}

async fn rejected_tokens(
    http: &reqwest::Client,
    base: &str,
) -> ConformanceResult<Vec<OperationId>> {
    Ok(http
        .get(format!("{base}/admin/v1/conformance/drive-tokens"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
