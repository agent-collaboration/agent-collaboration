//! Directory reads preserve native summaries and Registry-verified service identities.
#![cfg(feature = "conformance")]

mod annotation_setup;
use crate::{composed_fleet_fixture, native_fleet};
mod claim_routes;
use crate::configuration;
use crate::directory_session_fixture as support;
use crate::registry_setup;

use board_sync_protocol::BoardAnnotation;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use fleet_admission::{
    CopyMetadata, CopyMetadataAsOf, DirectoryEntry, DirectoryLease, DirectoryLeaseState,
    DirectoryQuery, DirectorySide, LeaseMetadata,
};
use fleet_protocol::*;
use message_board::{BoardId, RecordTime, ServiceId, WorkspaceId};
use std::{path::Path, time::Duration};

pub(super) const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440d83";
pub(super) const PEER: &str = "019f0000-0000-7000-8000-000000001d83";

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn directory_reads_real_copy_annotations_and_the_verified_holder_after_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let databases = [
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
        DatabaseId::new(durable_steps::Uuid::now_v7())?,
    ];
    let server = annotation_setup::start(root.path(), &jwks.url, annotation_setup::NOW).await?;
    annotation_setup::import_graph(&server, &workspace, &board, &databases).await?;
    let mut machine = support::admit(&server).await?;
    annotation_setup::create_projects(&mut machine.socket, &workspace, &board, &databases[..2])
        .await?;
    for database in &databases[..2] {
        annotation_setup::register(&mut machine.socket, *database).await?;
    }
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
            .await.map_err(|error|format!("Directory annotation Piece for {database}: {error}"))?,
            OperationAnswer::Replication(ReplicationAnswer::Applied { through: actual }) if actual == through
        ));
    }
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;

    annotation_setup::admit_imports(root.path(), &databases[2..]).await?;
    let server = annotation_setup::start(root.path(), &jwks.url, annotation_setup::NOW).await?;
    let mut machine = support::connect(&server)
        .await
        .map_err(|error| format!("Directory import reopen: {error}"))?;
    annotation_setup::await_imports(&mut machine.socket, &databases[2..])
        .await
        .map_err(|error| format!("Directory import readiness: {error}"))?;
    let retired_at = annotation_setup::retire(&server, databases[3]).await?;
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;

    // Observe committed public snapshots without running another publisher or alarm.
    let observed =
        support::read_committed_owners(root.path(), configuration::registry_config()?, &databases)
            .await?;
    let expired_at = annotation_setup::NOW
        + i64::try_from(
            configuration::registry_config()?
                .lease
                .lease_duration
                .as_millis(),
        )?
        + 1;
    let server = annotation_setup::start(root.path(), &jwks.url, expired_at).await?;
    let mut machine = support::connect(&server)
        .await
        .map_err(|error| format!("Directory expired-lease reopen: {error}"))?;
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
