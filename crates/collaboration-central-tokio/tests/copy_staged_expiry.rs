//! The original staged-part expiry obligation survives a whole store reopen.
#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "copy_receiver/mod.rs"]
mod support;

use board_sync_protocol::BoardAnnotation;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    Piece, PieceContents, PieceDigest, ReplicatedTransaction, ReplicationAnswer, TransactionKey,
    TransactionPart,
};
use sqlx_core::{query::query, row::Row};
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult, position};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_reopened_copy_alarm_removes_expired_staged_parts() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let prepared = async {
        fixture.create(false).await?;
        let statements = [
            "CREATE TABLE alarm_records(id TEXT NOT NULL PRIMARY KEY)",
            "CREATE INDEX alarm_records_index ON alarm_records(id)",
        ];
        let whole = ReplicatedTransaction::SchemaStep {
            commit_id: position(1)?.sequence,
            migration_version: 1,
            statements: statements
                .iter()
                .map(|statement| (*statement).to_owned())
                .collect(),
        };
        let part = Piece::<BoardAnnotation>::new(PieceContents {
            database: fixture.database,
            epoch: position(0)?.epoch,
            continues_from: position(0)?,
            through: position(1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: position(1)?.sequence,
                migration_version: 1,
                statements: vec![statements[0].to_owned()],
            }],
            part: Some(TransactionPart::new(
                TransactionKey {
                    epoch: position(0)?.epoch,
                    commit_id: position(1)?.sequence,
                },
                0,
                2,
                PieceDigest::of_transaction(&whole)?,
            )?),
            annotation: None,
        })?;
        let answer = fixture.apply(part).await?;
        let staged = staged_count(&fixture).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((answer, staged))
    }
    .await;
    fixture.objects.close().await?;
    let (answer, staged) = prepared?;
    assert!(
        matches!(answer, ReplicationAnswer::PartStaged { index: 0, acked, .. } if acked == position(0)?)
    );
    assert_eq!(
        staged, 1,
        "the fixture must persist an actual incomplete transaction"
    );
    drop(fixture);

    let reopened = Fixture::open(root.path()).await?;
    let observed = async {
        let before = staged_count(&reopened).await?;
        reopened.io.time.fetch_add(2 * 86_400_000, Ordering::SeqCst);
        reopened.runtime.on_alarm().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((before, staged_count(&reopened).await?))
    }
    .await;
    reopened.objects.close().await?;
    let (before, after) = observed?;
    assert_eq!(
        before, 1,
        "reopen must preserve the original staged transaction"
    );
    assert_eq!(
        after, 0,
        "the copy alarm must remove a part older than one day"
    );
    Ok(())
}

async fn staged_count(fixture: &Fixture) -> TestResult<i64> {
    Ok(fixture
        .copy
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS parts FROM _fleet_sync_staged_parts",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get("parts").map_err(StoreError::backend)
            })
        })
        .await?)
}
