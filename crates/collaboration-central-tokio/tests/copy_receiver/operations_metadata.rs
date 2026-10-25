//! Central bookkeeping is local, and cannot be claimed by replicated schema.
use super::{
    schema_piece,
    support::{Fixture, TestResult, position},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    Piece, PieceContents, ReplicatedTransaction, ReplicationAnswer, SchemaEntry, SchemaObjectKind,
    SnapshotManifest, SnapshotManifestAnswer,
};
use sqlx_core::{query::query, row::Row};

async fn manifest(fixture: &Fixture) -> TestResult<SnapshotManifest> {
    let answer = fixture
        .copy
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_receiver::snapshot_manifest(
                    scope,
                    position(1).map_err(StoreError::backend)?,
                )
                .await
                .map_err(fleet_sync_receiver::receiver_rollback)
            })
        })
        .await?;
    let SnapshotManifestAnswer::Manifest(manifest) = answer else {
        return Err("real Copy did not return its snapshot manifest".into());
    };
    Ok(manifest)
}
#[tokio::test]
async fn own_history_and_failure_tables_are_excluded_and_cannot_be_claimed_by_project_schema()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.create(false).await?;
        fixture.apply(schema_piece(fixture.database)?).await?;
        let owned_tables = fixture.copy.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM sqlite_schema WHERE type='table' AND name IN ('central_schema_migrations','central_schema_migration_failures')")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<i64,_>("count").map_err(StoreError::backend)
        })).await?;
        if owned_tables != 2 {return Err("Copy prelude did not create both own metadata tables before admission".into());}
        let before = manifest(&fixture).await?;
        let expected = fleet_sync_protocol::schema_fingerprint(&[SchemaEntry {
            kind: SchemaObjectKind::Table,
            name: "board_rows".into(),
            sql: "CREATE TABLE board_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)".into(),
        }])?;
        if before.schema.fingerprint != expected || before.tables.len() != 1
            || before.tables[0].table.as_str() != "board_rows" {
            return Err("central metadata entered the replicated catalog or fingerprint".into());
        }
        for table in ["central_schema_migrations", "central_schema_migration_failures"] {
            let piece = Piece::new(PieceContents {
                database: fixture.database,
                epoch: position(1)?.epoch,
                continues_from: position(1)?, through: position(2)?,
                transactions: vec![ReplicatedTransaction::SchemaStep {
                    commit_id: position(2)?.sequence, migration_version: 2,
                    statements: vec![
                        "CREATE TABLE must_rollback (id TEXT NOT NULL PRIMARY KEY)".into(),
                        format!("CREATE TABLE {table}(id TEXT NOT NULL PRIMARY KEY)"),
                    ],
                }], part: None, annotation: None,
            })?;
            let answer = fixture.apply(piece).await?;
            if !matches!(&answer, ReplicationAnswer::CopyUnavailable {
                reason: fleet_sync_protocol::CopyUnavailableReason::Damaged { detail }
            } if detail.contains(&format!("replicated schema conflicts with local object {table}"))) {
                return Err(format!("collision on central's own table {table} returned {answer:?}").into());
            }
            if manifest(&fixture).await? != before {
                return Err("refused collision changed the acknowledged schema/rows/position".into());
            }
        }
        let leaked = fixture.copy.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM sqlite_schema WHERE name='must_rollback'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<i64,_>("count").map_err(StoreError::backend)
        })).await?;
        if leaked != 0 {return Err("collision refusal leaked an earlier schema statement".into());}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
