//! Central and writer files both survive the loss of all native runtime objects.
#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::{
    CallId,
    storage::{CallReach, StepRecord, StepState},
};
use fleet_sync_lease::WriterState;
use fleet_sync_protocol::{
    Epoch, FrameId, PeerId, ReleaseAnswer, ReleaseRequest, ServerCall, ServerCallAnswer, TransferId,
};
use fleet_sync_writer::WriteRefusal;
use serde_json::{Value, json};
use sqlx_core::executor::Executor;
use std::{path::Path, time::Duration};
use tokio::task::JoinSet;
#[path = "native_http/support.rs"]
mod configuration;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/holder_writer_retry.rs"]
mod writer_fixture;

struct FrozenRelease {
    peer: PeerId,
    identity: CallId,
    frame: FrameId,
    request: ReleaseRequest,
    answer: ReleaseAnswer,
}

#[tokio::test]
async fn the_reopened_central_retries_its_stored_call_against_the_reopened_released_writer()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let settings = || {
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            root: root.path().join("central"),
            access_policy: fixture_policy(),
            jwks_url: jwks.url.clone(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
    };
    let writer_path = root.path().join("writer.db");
    let first = TokioServer::start(settings()?).await?;
    let prepared = freeze_and_lose_answer(first.base_url(), &writer_path).await;
    let stopped = first.shutdown().await;
    stopped?;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    // This creates fresh CentralPlatform, NativeObjects, ObjectRuntime, wake
    // owners and session attachments against the same committed files.
    let second = TokioServer::start(settings()?).await?;
    let proof = resume(second.base_url(), &writer_path, prepared).await;
    let stopped = second.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}

async fn freeze_and_lose_answer(base: &str, path: &Path) -> ConformanceResult<FrozenRelease> {
    let client = ConformanceClient::new(base)?;
    let mut peer = enroll_admitted_peer(
        &client,
        writer_fixture::SERVICE,
        writer_fixture::PEER,
        writer_fixture::CREDENTIAL,
        writer_fixture::INITIAL_SESSION,
    )
    .await?;
    let holder = PeerId::new(peer.peer)?;
    let writer = writer_fixture::writer(path, holder).await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut callers = JoinSet::new();
    let prepared = async {
        let mut write = writer.begin_write().await?;
        (&mut write).execute("INSERT INTO holder_records(record_id,amount) VALUES ('019f0000-0000-7000-8000-000000000704',41)").await?;
        write.commit().await?;
        let position = writer.snapshot().await?.position();
        let request = ReleaseRequest {
            database: writer_fixture::DATABASE.to_owned().try_into()?, epoch: Epoch::new(1)?,
            transfer: TransferId::try_from("019f0000-0000-7000-8000-000000000951".to_owned())?,
        };
        writer_fixture::start(&http, base, &mut callers, json!({
            "attempt":"019f0000-0000-7000-8000-000000000952", "holder":holder,
            "request":{"kind":"release","value":request}
        }));
        let (frame, received) = writer_fixture::received(&mut peer.socket).await
            .map_err(|error| format!("initial central did not send its admitted call: {error}"))?;
        let ServerCall::Release { call: identity, request: received } = received else {
            return Err("central did not send the release before restart".into());
        };
        if received != request { return Err("central changed the release request".into()); }
        let answer = writer.request_release(&request, identity.clone()).await?;
        if answer != (ReleaseAnswer::Released { final_position: position }) {
            return Err("real writer did not freeze the committed capture position".into());
        }
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(FrozenRelease {
            peer: holder, identity, frame, request, answer,
        })
    }.await;
    writer.close().await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let result = async {
        let prepared = prepared?; closed?;
        writer_fixture::pending_reply(&mut callers).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = writer_fixture::read_status(&http, base, prepared.identity.operation).await?;
                if let Some(step) = status.pointer("/value/step").filter(|value| !value.is_null()) {
                    let step: StepRecord = serde_json::from_value(step.clone())?;
                    if matches!(step.state, StepState::CallPending { attempt, .. } if attempt.reach == CallReach::MayHaveReached) {
                        return Ok::<_, Box<dyn std::error::Error+Send+Sync>>(());
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await??;
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(prepared)
    }.await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    result
}

async fn resume(base: &str, path: &Path, frozen: FrozenRelease) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut peer = enroll_admitted_peer(
        &client,
        writer_fixture::SERVICE,
        writer_fixture::PEER,
        writer_fixture::CREDENTIAL,
        writer_fixture::RECONNECTED_SESSION,
    )
    .await?;
    let writer = writer_fixture::writer(path, frozen.peer).await?;
    let result = async {
        let ReleaseAnswer::Released { final_position } = &frozen.answer else {
            return Err("restart fixture lost its frozen release answer".into());
        };
        if PeerId::new(peer.peer)? != frozen.peer
            || !matches!(writer.writer_state().await?, WriterState::Released { .. })
            || !matches!(writer.begin_write().await, Err(WriteRefusal::Released))
            || writer_fixture::amount(&writer).await? != 41 {
            return Err("reopened enrollment/writer lost its identity, release or committed row".into());
        }
        if writer.snapshot().await?.position() != *final_position {
            return Err("writer reopen advanced the frozen capture position".into());
        }
        // No conformance drive is invoked: the reopened Registry's normal
        // startup and persisted journal wake dispatch the still-pending call.
        let (frame, received) = writer_fixture::received(&mut peer.socket).await
            .map_err(|error| format!("reopened central did not retry its persisted call: {error}"))?;
        let ServerCall::Release { call, request } = received else {
            return Err("reopened central did not retry its actual holder call".into());
        };
        if call != frozen.identity || request != frozen.request || frame == frozen.frame {
            return Err("central restart changed the durable call/request or reused its old frame".into());
        }
        let answer = writer.request_release(&request, call.clone()).await?;
        if answer != frozen.answer || writer_fixture::amount(&writer).await? != 41
            || writer.snapshot().await?.position() != *final_position {
            return Err("reopened real writer did not replay its frozen receipt".into());
        }
        writer_fixture::send(&mut peer.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
        let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build()?;
        let expected = json!({"kind":"completed","value":{"kind":"release","value":answer}});
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actual = writer_fixture::read_status(&http, base, call.operation).await?;
                if actual == expected { return Ok::<_, Box<dyn std::error::Error+Send+Sync>>(()); }
                if actual.get("kind").and_then(Value::as_str) != Some("pending") {
                    return Err(format!("reopened caller did not converge: {actual}").into());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await??;
        println!("NATIVE_REOPEN_CASE whole_central_server_and_writer_reopen_preserve_the_call_and_frozen_answer passed");
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    writer.close().await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}
