//! Missing committed enrollment mappings must never reach the object transport.

use super::holder_writer_retry::{owner_post, read_status, received, send};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use durable_steps::{
    OperationId,
    storage::{CallReach, StepRecord, StepState},
};
use fleet_sync_protocol::{ReleaseAnswer, ServerCall, ServerCallAnswer};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinSet;

pub async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let service = "550e8400-e29b-41d4-a716-4466554400e2";
    let mut peer = enroll_admitted_peer(
        &client,
        service,
        "019f0000-0000-7000-8000-000000001023",
        "holder-mapping-carryover",
        "019f0000-0000-7000-8000-000000000931",
    )
    .await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut calls = JoinSet::new();
    let result = async {
        if !dispatches(&http, base).await?.is_empty() {
            return Err("Registry dispatch observer was not empty before its first holder call".into());
        }
        let mapped = input("019f0000-0000-7000-8000-000000000932", peer.peer.to_string());
        let http_call = http.clone(); let base_call = base.to_owned();
        calls.spawn(async move { owner_post(&http_call, &base_call, "holder-call", mapped).await });
        let (frame, request) = received(&mut peer.socket).await?;
        let ServerCall::Release { call, request } = request else {
            return Err("mapped holder did not receive the normal Release".into());
        };
        let answer = ReleaseAnswer::Released {
            final_position: fleet_sync_protocol::ReplicationPosition::new(
                request.epoch, fleet_sync_protocol::CommitSequence::new(0)
            )
        };
        send(&mut peer.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
        let expected = json!({"kind":"completed","value":{"kind":"release","value":answer}});
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actual = read_status(&http, base, call.operation).await?;
                if actual == expected { return Ok::<_, Box<dyn std::error::Error+Send+Sync>>(()); }
                if actual.get("kind").and_then(Value::as_str) != Some("pending") {
                    return Err(format!("mapped call did not complete: {actual}").into());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await??;
        let expected_dispatch = vec![ObjectAddress::Connection(message_board::ServiceId::try_from(service.to_owned())?)];
        if dispatches(&http, base).await? != expected_dispatch {
            return Err("mapped PeerId did not dispatch exactly once to its unequal committed ServiceId".into());
        }
        let missing = owner_post(&http, base, "holder-call", input(
            "019f0000-0000-7000-8000-000000000933",
            "019f0000-0000-7000-8000-000000000934".into(),
        )).await?;
        if missing.get("kind").and_then(Value::as_str) != Some("pending") {
            return Err(format!("missing mapping fabricated a holder answer: {missing}").into());
        }
        let operation: OperationId = serde_json::from_value(
            missing.pointer("/value/operation").cloned().ok_or("missing mapping lost its caller operation")?
        )?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actual = read_status(&http, base, operation).await?;
                if let Some(step) = actual.pointer("/value/step").filter(|value| !value.is_null()) {
                    let step: StepRecord = serde_json::from_value(step.clone())?;
                    if let StepState::CallPending { attempt, .. } = step.state {
                        if attempt.reach != CallReach::NeverSent || attempt.retry_reason.is_some() {
                            return Err(format!("missing mapping did not remain NeverSent: {attempt:?}").into());
                        }
                        return Ok::<_, Box<dyn std::error::Error+Send+Sync>>(());
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await??;
        if dispatches(&http, base).await? != expected_dispatch {
            return Err("a holder without a committed mapping reached the object transport".into());
        }
        println!("HOLDER_MAPPING_CASE real_committed_lookup mapped_dispatches=1 missing_mapping_dispatches=0 missing_reach=never_sent passed");
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}

fn input(attempt: &str, holder: String) -> Value {
    json!({"attempt":attempt,"holder":holder,"request":{"kind":"release","value":{
        "database":"019f0000-0000-7000-8000-000000000935", "epoch":1,
        "transfer":"019f0000-0000-7000-8000-000000000936"
    }}})
}
async fn dispatches(http: &reqwest::Client, base: &str) -> ConformanceResult<Vec<ObjectAddress>> {
    Ok(http
        .get(format!("{base}/admin/v1/conformance/registry-dispatches"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
