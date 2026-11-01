//! Recreate the actual owned Worker process and observe its persisted forwarding state.

use super::{
    forwarded_post_lifecycle_oracles as lifecycle,
    forwarded_post_scenarios::{
        HOLDER_CREDENTIAL, HOLDER_SERVICE, RegisteredForwardingPeers, SENDER_CREDENTIAL,
        SENDER_SERVICE, control, registered_forwarding_peers,
    },
    forwarded_post_wire as wire,
    workerd_process::{WorkerdProcess, WorkerdSettings},
};
use board_sync_protocol::{BoardConsumerAnswer, BoardServerRequest, ForwardOutcome};
use collaboration_central_conformance::{ConformanceResult, RuntimeControl};
use collaboration_central_core::RegistryTime;
use fleet_sync_protocol::{DatabaseId, FrameId, OperationAnswer, ServerFrame, ServerRequest};
use reqwest_websocket::WebSocket;
use std::{path::Path, time::Duration};

struct PendingBeforeRestart {
    database: DatabaseId,
    request_frames: Vec<FrameId>,
    holder_frames: Vec<FrameId>,
    started: chrono::DateTime<chrono::Utc>,
}

pub(super) async fn run(settings: &WorkerdSettings) -> ConformanceResult<()> {
    let base = format!("http://127.0.0.1:{}", settings.port);
    let first = WorkerdProcess::start(settings).await?;
    let prepared = pending_before_restart(&base).await;
    let stopped = first.stop().await;
    let pending = prepared?;
    stopped?;
    drop(std::net::TcpListener::bind(("127.0.0.1", settings.port))?);
    inspect_stopped_storage(&settings.persistence, &pending.request_frames)?;

    let second = WorkerdProcess::start(settings).await?;
    let proof = after_restart(&base, pending).await;
    let stopped = second.stop().await;
    proof?;
    stopped?;
    println!(
        "FORWARD_REOPEN_CASE actual_process_recreation_persisted_slots_deadline_expiry_and_opaque_absence passed"
    );
    Ok(())
}

async fn pending_before_restart(base: &str) -> ConformanceResult<PendingBeforeRestart> {
    let RegisteredForwardingPeers {
        database,
        sender_socket: mut sender,
        holder_socket: mut holder,
    } = registered_forwarding_peers(
        base,
        Some(collaboration_central_conformance::fixture_time()?),
    )
    .await?;
    let proof = async {
        // Registry admission and both connection clocks share this instant,
        // so Hello's lease remains valid across the controlled restart interval.
        let started = collaboration_central_conformance::fixture_time()?.0;
        control(
            base,
            RuntimeControl::SetTime {
                now: RegistryTime(started),
            },
        )
        .await?;
        let mut request_frames = Vec::new();
        let mut holder_frames = Vec::new();
        for _ in 0..16 {
            request_frames.push(lifecycle::post(&mut sender, database).await?);
            holder_frames.push(lifecycle::holder_frame(&mut holder).await?);
        }
        // A controlled interval before restart distinguishes retaining the
        // original deadline from silently granting a fresh thirty seconds.
        control(
            base,
            RuntimeControl::SetTime {
                now: RegistryTime(started + chrono::TimeDelta::seconds(10)),
            },
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(PendingBeforeRestart {
            database,
            request_frames,
            holder_frames,
            started,
        })
    }
    .await;
    let sender_closed = wire::close(sender).await;
    let holder_closed = wire::close(holder).await;
    let pending = proof?;
    sender_closed?;
    holder_closed?;
    Ok(pending)
}

fn inspect_stopped_storage(root: &Path, requests: &[FrameId]) -> ConformanceResult<()> {
    let identity = requests
        .first()
        .ok_or("restart setup has no original request identity")?
        .to_string();
    let marker = lifecycle::POST_MARKER.as_bytes();
    let mut paths = vec![root.to_path_buf()];
    let mut files = 0_u64;
    let mut bytes_scanned = 0_u64;
    let mut observed_identity = false;
    while let Some(path) = paths.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                paths.push(entry?.path());
            }
        } else {
            let bytes = std::fs::read(&path)?;
            files += 1;
            bytes_scanned += bytes.len() as u64;
            if bytes.windows(marker.len()).any(|part| part == marker) {
                return Err("opaque forwarded post entered actual stopped Worker storage".into());
            }
            observed_identity |= bytes
                .windows(identity.len())
                .any(|part| part == identity.as_bytes());
        }
    }
    if !observed_identity {
        return Err(
            "stopped Worker storage scan did not reach the original pending correlation".into(),
        );
    }
    println!(
        "FORWARD_REOPEN_STORAGE files={files} bytes={bytes_scanned} original_identity_present=true opaque_marker_absent=true"
    );
    Ok(())
}

async fn after_restart(base: &str, pending: PendingBeforeRestart) -> ConformanceResult<()> {
    let restored_sender_time = RegistryTime(pending.started + chrono::TimeDelta::seconds(10));
    let mut sender = super::normal_session_support::reconnect_forwarding_peer(
        base,
        SENDER_SERVICE,
        SENDER_CREDENTIAL,
        restored_sender_time,
    )
    .await
    .map_err(|error| format!("reopened sender Welcome was not observed: {error}"))?;
    let mut holder = super::normal_session_support::reconnect_forwarding_peer(
        base,
        HOLDER_SERVICE,
        HOLDER_CREDENTIAL,
        RegistryTime(pending.started),
    )
    .await
    .map_err(|error| format!("reopened holder Welcome was not observed: {error}"))?;
    let proof = async {
        control(base, RuntimeControl::SetTime {
            now: restored_sender_time,
        }).await?;
        let overflow = lifecycle::post(&mut sender, pending.database).await?;
        if lifecycle::outcome(&mut sender, overflow).await? != ForwardOutcome::Overloaded {
            return Err("reopened Worker lost its sixteen persisted pending slots".into());
        }
        control(base, RuntimeControl::SetTime {
            now: RegistryTime(pending.started + chrono::TimeDelta::seconds(30)),
        }).await?;
        // Public capacity is the observation: with a pinned clock before all
        // fresh deadlines, sixteen unanswered fresh deliveries leave no old slot.
        // The first rejected attempt may precede the actual owning alarm.
        let (first_request, first_holder) = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let request = lifecycle::post(&mut sender, pending.database).await?;
                match delivery_or_overload(&mut sender, &mut holder, request).await? {
                    CapacityObservation::Overloaded => {}
                    CapacityObservation::Delivered(frame) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((request, frame)),
                }
            }
        }).await.map_err(|error| format!("expired Worker forwards did not release persisted capacity: {error}"))??;
        let mut fresh_holders = vec![first_holder];
        if pending.holder_frames.contains(&first_holder) {
            return Err("reopened Worker replayed an old holder identity as a fresh request".into());
        }
        for _ in 1..16 {
            let request = lifecycle::post(&mut sender, pending.database).await?;
            let CapacityObservation::Delivered(frame) = delivery_or_overload(&mut sender, &mut holder, request).await?
            else { return Err("expired Worker forwards left an old slot occupied during sixteen fresh deliveries".into()); };
            if pending.holder_frames.contains(&frame) || fresh_holders.contains(&frame) {
                return Err("fresh Worker forwarding slots reused an old or duplicate holder identity".into());
            }
            fresh_holders.push(frame);
        }
        let excess = lifecycle::post(&mut sender, pending.database).await?;
        if lifecycle::outcome(&mut sender, excess).await? != ForwardOutcome::Overloaded {
            return Err("reopened Worker did not retain all sixteen fresh pending slots".into());
        }
        // A current reply still has a usable callback and frees an actual slot.
        let reply = lifecycle::answer(&mut holder, first_holder).await?;
        if lifecycle::outcome(&mut sender, first_request).await? != (ForwardOutcome::Answered { reply }) {
            return Err("reopened holder lost the exact fresh correlation or opaque reply".into());
        }
        let replacement = lifecycle::post(&mut sender, pending.database).await?;
        let delivered = lifecycle::holder_frame(&mut holder).await?;
        if pending.holder_frames.contains(&delivered) || fresh_holders.contains(&delivered) {
            return Err("reopened terminal slot reuse retained a prior holder identity".into());
        }
        let reply = lifecycle::answer(&mut holder, delivered).await?;
        if lifecycle::outcome(&mut sender, replacement).await? != (ForwardOutcome::Answered { reply }) {
            return Err("reopened holder terminal did not free a usable exact forwarding slot".into());
        }
        println!("FORWARD_REOPEN_CAPACITY old_sixteen_retained_then_expired fresh_sixteen_delivered seventeenth_overloaded passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let sender_closed = wire::close(sender).await;
    let holder_closed = wire::close(holder).await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}

enum CapacityObservation {
    Overloaded,
    Delivered(FrameId),
}
async fn delivery_or_overload(
    sender: &mut WebSocket,
    holder: &mut WebSocket,
    request: FrameId,
) -> ConformanceResult<CapacityObservation> {
    tokio::select! {
        incoming = wire::next(sender) => {
            match incoming? {
                ServerFrame::Answer { frame, body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(ForwardOutcome::Overloaded)) } if frame == request => Ok(CapacityObservation::Overloaded),
                _ => Err("restart capacity retry received an uncorrelated or unexpected sender outcome".into()),
            }
        }
        incoming = wire::next(holder) => {
            match incoming? {
                ServerFrame::Request { frame, body: ServerRequest::Consumer(BoardServerRequest::ForwardedPost(_)) } => Ok(CapacityObservation::Delivered(frame)),
                _ => Err("restart capacity probe did not receive an actual holder delivery".into()),
            }
        }
    }
}
