//! Directory reads preserve native summaries and Registry-verified service identities.
#![cfg(feature = "conformance")]

#[path = "directory_session/claim_routes.rs"]
mod claim_routes;
#[path = "native_http/support.rs"]
mod configuration;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_fleet;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "directory_session/support.rs"]
mod support;

use board_sync_protocol::{
    BoardAnnotation, BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest,
    BoardPayload, BoardReadPage, BoardReadScope, FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_admission::{
    CopyMetadata, CopyMetadataAsOf, DirectoryEntry, DirectoryLease, DirectoryLeaseState,
    DirectoryQuery, DirectorySide, FleetOperation, FleetRequest, FleetRequestResult, LeaseMetadata,
};
use fleet_sync_protocol::*;
use message_board::{BoardId, ProjectId, RecordTime, ServiceId, WorkspaceId};
use std::{path::Path, time::Duration};

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440d83";
const PEER: &str = "019f0000-0000-7000-8000-000000001d83";

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn directory_reads_real_copy_annotations_and_the_verified_holder_after_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let config = configuration::registry_config()?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let databases = [
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
    ];
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::admit(&server).await?;
    support::create_graph(&mut machine.socket, &workspace, &board, &databases).await?;
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;

    // Public library tasks create genuine grants and physical Copies. The arranger's
    // admission gate is a setup stand-in, not proof of normal Register composition.
    for database in &databases[..2] {
        registry_setup::register_grant_history(
            root.path(),
            config,
            *database,
            vec![(Epoch::new(1)?, PeerId::new(machine.peer)?)],
        )
        .await?;
    }
    registry_setup::import_unheld(root.path(), config, databases[2]).await?;
    let retired_at = registry_setup::retire_unheld(
        root.path(),
        config,
        databases[3],
        PeerId::new(machine.peer)?,
    )
    .await?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::connect(&server).await?;
    let latest: RecordTime = serde_json::from_value(
        serde_json::json!({"kind":"router","value":"2026-10-18T10:00:00.123Z"}),
    )?;
    let annotations = [
        BoardAnnotation::ProjectSummary {
            latest_record_at: latest,
        },
        BoardAnnotation::ProjectSummary {
            latest_record_at: RecordTime::LegacyUnknown,
        },
    ];
    for (database, annotation) in databases.iter().zip(&annotations) {
        let through = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
        let piece = Piece::new(PieceContents {
            database: *database,
            epoch: Epoch::new(1)?,
            continues_from: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)),
            through,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(1),
                migration_version: 1,
                statements: vec![
                    "CREATE TABLE directory_rows(id TEXT NOT NULL PRIMARY KEY)".into(),
                ],
            }],
            part: None,
            annotation: Some(annotation.clone()),
        })?;
        assert!(matches!(
            support::answer(
                &mut machine.socket,
                OperationRequest::Replication(ReplicationRequest::Piece(piece))
            )
            .await?,
            OperationAnswer::Replication(ReplicationAnswer::Applied { through: actual }) if actual == through
        ));
    }
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;

    // Drive each actual owner's alarm to completion before restarting the native
    // server. This is operation-owned readiness, not a timing or polling assumption.
    let observed = support::publish(root.path(), config, &databases).await?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::connect(&server).await?;
    let proof = async {
        let entries = support::directory(&mut machine.socket, DirectoryQuery::default()).await?;
        assert_eq!(entries.len(), 4);
        for (index, database) in databases.iter().enumerate() {
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("Directory omitted source lease version")?;
            let DirectorySide::Known(metadata) = &entry.lease_side else {
                return Err("Published source lease version missing".into());
            };
            let revision = observed[index].1.revision;
            assert_eq!(metadata.lease_version.epoch().get(), revision.epoch.value());
            assert_eq!(metadata.lease_version.change(), revision.change.get());
        }
        for (index, annotation) in annotations.iter().enumerate() {
            let database = databases[index];
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("Directory omitted reported project")?;
            assert_eq!((&entry.workspace, &entry.board), (&workspace, &board));
            let BoardAnnotation::ProjectSummary { latest_record_at,
            } = annotation;
            let summary = observed[index]
                .0
                .as_ref()
                .ok_or("physical Copy summary missing")?;
            assert_eq!(
                entry.copy_side,
                DirectorySide::Known(CopyMetadata {
                    latest_record_at: *latest_record_at,
                    as_of: CopyMetadataAsOf {
                        position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1)),
                        acked_at: summary.acked_at
                    },
                })
            );
            let snapshot = &observed[index].1;
            let grant = snapshot.grant.as_ref().ok_or("public grant missing")?;
            assert!(matches!(&entry.lease_side,DirectorySide::Known(LeaseMetadata {
                lease:DirectoryLease::Held {epoch,holder,state:DirectoryLeaseState::Expired,expires_at},as_of,lease_version:_,
            }) if *epoch==grant.epoch && holder.as_str()==SERVICE && expires_at.timestamp_millis()==grant.expires_at.unix_millis() && as_of.timestamp_millis()==snapshot.changed_at.unix_millis()));
            assert_eq!(
                serde_json::from_slice::<DirectoryEntry>(&serde_json::to_vec(entry)?)?,
                *entry
            );
        }
        let unheld = entries
            .iter()
            .find(|entry| entry.project.as_str() == databases[2].to_string())
            .ok_or("Unheld project omitted")?;
        assert_eq!(unheld.copy_side, DirectorySide::NotYetReported);
        assert!(matches!(
            &unheld.lease_side,
            DirectorySide::Known(LeaseMetadata {
                lease: DirectoryLease::Unheld,
                ..
            })
        ));
        let retired = entries.iter().find(|entry|entry.project.as_str()==databases[3].to_string()).ok_or("Retired project omitted")?;
        assert_eq!(retired.copy_side,DirectorySide::NotYetReported);
        assert!(matches!(&retired.lease_side,DirectorySide::Known(LeaseMetadata {
            lease:DirectoryLease::Retired {retired_at:shown},as_of,lease_version:_,
        }) if shown.timestamp_millis()==retired_at.unix_millis() && as_of.timestamp_millis()==observed[3].1.changed_at.unix_millis()));
        assert_eq!(
            support::directory(
                &mut machine.socket,
                DirectoryQuery {
                    holder: Some(ServiceId::try_from(SERVICE.to_owned())?),
                    ..Default::default()
                }
            )
            .await?
            .len(),
            2
        );
        assert!(
            support::directory(
                &mut machine.socket,
                DirectoryQuery {
                    holder: Some(ServiceId::try_from(PEER.to_owned())?),
                    ..Default::default()
                }
            )
            .await?
            .is_empty(),
            "a peer identifier must never act as the enrolled service identifier"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
