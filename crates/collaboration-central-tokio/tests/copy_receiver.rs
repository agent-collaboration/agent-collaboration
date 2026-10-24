//! Copy hooks run inside the real apply/publication transaction.
#![allow(clippy::panic_in_result_fn)]
#[path = "copy_receiver/mod.rs"]
mod support;
use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{Piece, PieceContents, ReplicatedTransaction, ReplicationAnswer};
use message_board::RecordTime;
use serde_json::json;
use support::{Fixture, TestResult, position};

#[tokio::test]
async fn applied_summary_keeps_unknown_time_and_no_status_in_copy_and_directory() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        let annotation = BoardAnnotation::ProjectSummary {
            status: None,
            latest_record_at: RecordTime::LegacyUnknown,
        };
        let piece = Piece::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(0)?,
            through: position(1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: position(1)?.sequence,
                migration_version: 1,
                statements: vec!["CREATE TABLE board_rows(id TEXT NOT NULL PRIMARY KEY, value TEXT NOT NULL)".into()],
            }],
            part: None,
            annotation: Some(annotation.clone()),
        })?;
        assert_eq!(fixture.apply(piece).await?, ReplicationAnswer::Applied { through: position(1)? });
        assert_eq!(fixture.summary(false).await?, Some(json!({
            "database":fixture.database,"summary":annotation,"position":position(1)?,"ackedAt":1_792_281_600_000_i64
        })));
        fixture.runtime.on_alarm().await?;
        assert_eq!(fixture.summary(true).await?, fixture.summary(false).await?);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[path = "copy_receiver/retry.rs"]
mod retry;
#[path = "copy_receiver/rollback.rs"]
mod rollback;
#[path = "copy_receiver/snapshot.rs"]
mod snapshot;

fn schema_piece(database: fleet_sync_protocol::DatabaseId) -> TestResult<Piece<BoardAnnotation>> {
    Ok(Piece::new(PieceContents {
        database,
        epoch: position(0)?.epoch,
        continues_from: position(0)?,
        through: position(1)?,
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: position(1)?.sequence,
            migration_version: 1,
            statements: vec![
                "CREATE TABLE board_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)".into(),
            ],
        }],
        part: None,
        annotation: Some(BoardAnnotation::ProjectSummary {
            status: Some(message_board::StatusName::try_from("active".to_owned())?),
            latest_record_at: RecordTime::Router("2026-10-18T12:00:00Z".parse()?),
        }),
    })?)
}

#[tokio::test]
async fn a_piece_without_annotation_keeps_the_last_summary_and_admits_no_new_update() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        fixture.apply(schema_piece(fixture.database)?).await?;
        fixture.runtime.on_alarm().await?;
        let summary = fixture.runtime.copy_summary().await?;
        let counts = fixture.counts().await?;
        let calls = fixture.io.calls.lock().map_err(|_| "calls poisoned")?.len();
        let piece = Piece::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(1)?,
            through: position(2)?,
            transactions: vec![ReplicatedTransaction::Rows {
                commit_id: position(2)?.sequence,
                rows: vec![],
            }],
            part: None,
            annotation: None,
        })?;
        assert_eq!(
            fixture.apply(piece).await?,
            ReplicationAnswer::Applied {
                through: position(2)?
            }
        );
        fixture.runtime.on_alarm().await?;
        assert_eq!(fixture.runtime.copy_summary().await?, summary);
        assert_eq!(fixture.counts().await?, counts);
        assert_eq!(
            fixture.io.calls.lock().map_err(|_| "calls poisoned")?.len(),
            calls
        );
        assert_eq!(fixture.summary(true).await?, fixture.summary(false).await?);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[path = "copy_receiver/capacity.rs"]
mod capacity;

#[path = "copy_receiver/operations_metadata.rs"]
mod operations_metadata;

#[path = "copy_receiver/owner_repair.rs"]
mod owner_repair;
