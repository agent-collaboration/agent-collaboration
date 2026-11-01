//! The full configured attempt expires before the caller's original drive lease.

use super::holder_writer_retry::{
    pending_reply, read_status, received, received_within, send, start,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{
    FrameId, ReleaseAnswer, ReplicationPosition, ServerCall, ServerCallAnswer,
};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::task::JoinSet;

pub const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400d4";

pub async fn run<Observe, Observed, Deadline, Armed>(
    base: &str,
    mut observe: Observe,
    mut deadline: Deadline,
) -> ConformanceResult<()>
where
    Observe: FnMut() -> Observed,
    Observed: Future<Output = ConformanceResult<Vec<FrameId>>>,
    Deadline: FnMut() -> Armed,
    Armed: Future<Output = ConformanceResult<Option<collaboration_central_core::UnixMillis>>>,
{
    let client = ConformanceClient::new(base)?;
    let started = collaboration_central_conformance::fixture_time()?;
    let connection = collaboration_central_core::ObjectAddress::Connection(
        message_board::ServiceId::try_from(SERVICE.to_owned())?,
    );
    client
        .set_object_time(collaboration_central_core::ObjectAddress::Registry, started)
        .await?;
    let mut peer = enroll_admitted_peer(
        &client,
        SERVICE,
        "019f0000-0000-7000-8000-000000001022",
        "holder-full-wait",
        "019f0000-0000-7000-8000-000000000801",
    )
    .await?;
    client.set_object_time(connection.clone(), started).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(35))
        .build()?;
    let mut tasks = JoinSet::new();
    start(
        &http,
        base,
        &mut tasks,
        json!({"attempt":"019f0000-0000-7000-8000-000000000802","holder":peer.peer,"request":{"kind":"release","value":{"database":"019f0000-0000-7000-8000-000000000803","epoch":1,"transfer":"019f0000-0000-7000-8000-000000000804"}}}),
    );
    let result = async {
        let (original_frame, first) = received(&mut peer.socket).await
            .map_err(|error| format!("full-wait initial Release was not observed: {error}"))?;
        let ServerCall::Release { call: original_call, request: original_request } = first else { return Err("full-wait caller did not send Release".into()); };
        if observe().await? != vec![original_frame] { return Err("full-wait first call did not own exactly one waiter".into()); }
        let armed = deadline().await?.ok_or("full-wait call has no armed deadline")?;
        let elapsed = armed.0 - started.0.timestamp_millis();
        if !(19_999..30_000).contains(&elapsed) {
            return Err(format!("full configured wait did not finish inside the original 30-second drive lease: {elapsed:?}").into());
        }
        if elapsed != 20_000 {
            return Err("the actual armed holder deadline differs from the configured 20 seconds".into());
        }
        // No answer is sent. The real wake owner must end the entire configured
        // 20-second attempt before a replacement drive is needed at 30 seconds.
        let expires_at = collaboration_central_core::RegistryTime(
            chrono::DateTime::from_timestamp_millis(armed.0).ok_or("holder fixture deadline")?,
        );
        client.set_object_time(collaboration_central_core::ObjectAddress::Registry, expires_at).await?;
        client.set_object_time(connection.clone(), expires_at).await?;
        let (retry_frame, retry) = received_within(&mut peer.socket, Duration::from_secs(30)).await
            .map_err(|error| format!("full-wait retry after the actual armed deadline was not observed: {error}"))?;
        let ServerCall::Release { call, request } = retry else { return Err("full-wait retry did not send Release".into()); };
        if call != original_call || request != original_request || retry_frame == original_frame { return Err("full-wait retry changed its durable call or reused a frame".into()); }
        let frames = observe().await?;
        if frames != vec![retry_frame] { return Err(format!("full-wait retry retained an obsolete/overlapping waiter: {frames:?}").into()); }
        pending_reply(&mut tasks).await?;
        // This is a clock-delimited interleaving, not a readiness delay: the
        // replacement call deliberately remains unanswered past the old lease.
        let past_original_drive = collaboration_central_core::RegistryTime(started.0 + chrono::TimeDelta::seconds(31));
        client.set_object_time(collaboration_central_core::ObjectAddress::Registry, past_original_drive).await?;
        client.set_object_time(connection, past_original_drive).await?;
        if observe().await? != vec![retry_frame] {
            return Err("crossing the original drive expiry lost or duplicated the replacement waiter".into());
        }
        let answer = ReleaseAnswer::Released { final_position: ReplicationPosition::new(request.epoch, fleet_sync_protocol::CommitSequence::new(0)) };
        send(&mut peer.socket, retry_frame, ServerCallAnswer::Release(answer.clone())).await?;
        let expected = json!({"kind":"completed","value":{"kind":"release","value":answer}});
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = read_status(&http, base, original_call.operation).await?;
                if state == expected { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()); }
                if state.get("kind").and_then(Value::as_str) != Some("pending") { return Err(format!("full-wait retry did not complete: {state}").into()); }
            }
        }).await??;
        if !observe().await?.is_empty() { return Err("completed full-wait call leaked its waiter".into()); }
        println!("HOLDER_FULL_WAIT_CASE configured_20s_attempt_inside_30s_drive calls=2 observed_live_waiters=1 obsolete_waiters=0 original_drive_expiry_crossed=true first_attempt_ms={elapsed} passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}
