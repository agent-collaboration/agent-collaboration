//! Owner HTTP composes the real Registry tasks, Copy fences and admitted peers.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;
#[path = "owner_registry/fixture.rs"]
mod fixture;
#[path = "owner_registry/grant_refusals.rs"]
mod grant_refusals;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_fleet;
#[path = "owner_registry/notices.rs"]
mod notices;
#[path = "owner_registry/preview_presence.rs"]
mod preview_presence;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "owner_registry/retirement.rs"]
mod retirement;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "directory_session/support.rs"]
mod support;

use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload, BoardReadPage,
    BoardReadScope, FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
    owner_assertion,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fixture::{Fixture, confirm, refused};
use fleet_admission::{
    DirectoryEntry, DirectoryQuery, FleetOperation, FleetRequest, FleetRequestResult,
};
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, ProjectId, WorkspaceId};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440d83";
const PEER: &str = "019f0000-0000-7000-8000-000000001d83";
const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440583";
const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000005583";

#[tokio::test]
async fn owner_takeover_round_trips_the_native_preview_refuses_tampering_and_replays_the_grant()
-> ConformanceResult<()> {
    let fixture = Fixture::open().await?;
    let proof = async {
        let preview = fixture.preview().await?;
        let decoded: TakeoverPreview = serde_json::from_value(preview.clone())?;
        if decoded.holder != PeerId::try_from(PEER.to_owned())?
            || decoded.fence != Epoch::new(1)?
            || decoded.acked != fixture::position(1, 0)?
            || decoded.last_seen.is_none()
        {
            return Err(
                "preview did not expose the actual holder, position and renewal observation".into(),
            );
        }
        // Every original native field, including the opaque token, is submitted unchanged.
        for field in ["holder", "fence", "acked"] {
            let mut tampered = preview.clone();
            tampered[field] = match field {
                "holder" => json!(TARGET_PEER),
                "fence" => json!(2),
                _ => json!({"epoch":1,"sequence":1}),
            };
            let offered = confirm(durable_steps::Uuid::now_v7(), tampered);
            refused(&fixture.post("takeover", &offered).await?, "previewStale")?;
        }
        let request = confirm(durable_steps::Uuid::now_v7(), preview);
        let granted = fixture.post("takeover", &request).await?;
        fixture::granted(&granted, "granted", 2, fixture::position(1, 0)?)?;
        if fixture.post("takeover", &request).await? != granted {
            return Err("same-ID takeover did not replay its exact stored grant".into());
        }
        let mut conflict = request;
        conflict["reason"] = json!("different content under the existing transfer");
        if fixture.post("takeover", &conflict).await?.get("kind")
            != Some(&json!("conflictingRequest"))
        {
            return Err("takeover reused an ID with different content".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let snapshot = fixture::snapshot(root.path(), database).await?;
    let grant = snapshot
        .grant
        .ok_or("granted takeover has no durable library grant")?;
    if grant.epoch != Epoch::new(2)? || grant.holder != PeerId::try_from(TARGET_PEER.to_owned())? {
        return Err("owner takeover did not commit the target's unequal installation peer".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_takeover_refuses_a_preview_made_stale_by_a_real_committed_piece()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof = async {
        let preview = fixture.preview().await?;
        let piece = Piece::<board_sync_protocol::BoardAnnotation>::new(PieceContents {
            database:fixture.database,epoch:Epoch::new(1)?,
            continues_from:fixture::position(1,0)?,through:fixture::position(1,1)?,
            transactions:vec![ReplicatedTransaction::SchemaStep {
                commit_id:CommitSequence::new(1),migration_version:1,
                statements:vec!["CREATE TABLE owner_takeover_rows(id TEXT NOT NULL PRIMARY KEY)".into()],
            }],part:None,annotation:None,
        })?;
        if !matches!(support::answer(&mut fixture.old,OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?,
            OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==fixture::position(1,1)?) {
            return Err("stale-preview setup did not commit through the normal holder session".into());
        }
        refused(&fixture.post("takeover", &confirm(durable_steps::Uuid::now_v7(),preview)).await?, "previewStale")?;
        if !matches!(support::answer(&mut fixture.old,OperationRequest::Replication(ReplicationRequest::CopyStatus {database:fixture.database})).await?,
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)))
                if status.fence==Epoch::new(1)? && status.acked==fixture::position(1,1)?) {
            return Err("refused stale preview changed the real Copy fence or committed prefix".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let grant = fixture::snapshot(root.path(), database)
        .await?
        .grant
        .ok_or("old grant disappeared")?;
    if grant.epoch != Epoch::new(1)? || grant.holder != PeerId::try_from(PEER.to_owned())? {
        return Err("stale preview nevertheless committed a new Registry grant".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_move_uses_the_admitted_holders_release_before_grant_and_replays_its_original_epoch()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let request = json!({"transferId":durable_steps::Uuid::now_v7(),"toInstallation":TARGET_SERVICE,"reason":"owner move real release"});
    let mut pending = tokio::task::JoinSet::new();
    let http = fixture.http.clone();
    let route = fixture.route("move");
    let sent = request.clone();
    pending.spawn(async move { fixture::post(&http, &route, &sent).await });
    let proof = async {
        let mut early_reply = None;
        let (frame, release) = loop {
            tokio::select! {
                released = fixture::release(&mut fixture.old) => break released?,
                ended = pending.join_next(), if early_reply.is_none() => {
                    let reply = ended.ok_or("owner Move caller vanished before Release")???;
                    if reply.get("kind") != Some(&json!("pending")) {
                        return Err(format!("owner Move ended before the real holder released: {reply}").into());
                    }
                    early_reply = Some(reply);
                }
            }
        };
        if release.database != fixture.database
            || release.epoch != Epoch::new(1)?
            || serde_json::to_value(release.transfer)? != request["transferId"]
        {
            return Err("owner Move released another project, epoch or attempt".into());
        }
        fixture
            .old
            .send(Message::Binary(
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
                    frame,
                    body: ClientAnswer::Crate(ServerCallAnswer::Release(ReleaseAnswer::Released {
                        final_position: fixture::position(1, 0)?,
                    })),
                })?
                .into(),
            ))
            .await?;
        let completed = match early_reply {
            Some(reply) => reply,
            None => tokio::time::timeout(Duration::from_secs(8), pending.join_next())
                .await?
                .ok_or("owner Move HTTP caller vanished")???,
        };
        let moved = fixture.complete("move", &request, completed).await?;
        fixture::granted(&moved, "moved", 2, fixture::position(1, 0)?)?;
        if fixture.post("move", &request).await? != moved {
            return Err(
                "same-ID Move recomputed its from_epoch or changed the stored result".into(),
            );
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    pending.abort_all();
    while pending.join_next().await.is_some() {}
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let grant = fixture::snapshot(root.path(), database)
        .await?
        .grant
        .ok_or("Move did not keep a real grant")?;
    if grant.epoch != Epoch::new(2)? || grant.holder != PeerId::try_from(TARGET_PEER.to_owned())? {
        return Err("owner Move did not durably grant the target after release/fencing".into());
    }
    Ok(())
}
