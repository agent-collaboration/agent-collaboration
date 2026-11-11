//! A predecessor Router's interaction store opens with the pinned library: its
//! released durable-steps journal is adopted in the owner transaction, admitted
//! in exactly the converted shape, and keeps every row. A tampered or
//! half-converted journal is refused without rewriting the file.
use super::*;
use crate::schema_prefix_fixture::{PrecedingJournal, create_native_prefix_with_journal};
use sqlx::Connection;

/// Router 0.1.125's last interaction migration.
const ROUTER_0125_NATIVE: i64 = 202610310002;
const OPERATION: &str = "01a1c1a0-0000-7000-8000-000000000001";

async fn writer(path: &std::path::Path) -> sqlx_turso::TursoConnection {
    sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(path)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

/// A predecessor's file holding one finished journal operation with one step.
async fn predecessor(journal: PrecedingJournal) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    create_native_prefix_with_journal(&path, ROUTER_0125_NATIVE, journal).await;
    let mut connection = writer(&path).await;
    sqlx::query("INSERT INTO _fleet_sync_operations(operation_id,kind,kind_version,request_key,input,input_digest,state,result,failure,finished_at,next_attempt_at,attempt) VALUES(?,'fixture.kind',1,'request-1',x'7b7d',x'0102','completed',x'7b7d',NULL,1794000005000,NULL,1)")
        .bind(OPERATION).execute(&mut connection).await.unwrap();
    // Journal 0002's steps carry their own NOT NULL metadata; 0001's don't.
    let step = if matches!(journal, PrecedingJournal::MarkerV1) {
        "INSERT INTO _fleet_sync_steps(operation_id,position,step_name,kind,intent,output,first_attempt_at,may_have_reached) VALUES(?,0,'write','effect','{}',x'7b7d',1794000004000,1)"
    } else {
        "INSERT INTO _fleet_sync_steps(operation_id,position,step_name,kind,intent,output,first_attempt_at,may_have_reached,created_at,updated_at) VALUES(?,0,'write','effect','{}',x'7b7d',1794000004000,1,1794000004000,1794000006000)"
    };
    sqlx::query(sqlx::AssertSqlSafe(step))
        .bind(OPERATION)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    (directory, path)
}

type CarriedStep = (String, i64, String, String, Option<i64>, Option<i64>);

async fn journal_state(
    connection: &mut sqlx_turso::TursoConnection,
) -> (Vec<String>, Vec<i64>, String, CarriedStep) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name IN ('_fleet_sync_migrations','_fleet_sync_operations','_fleet_sync_steps','_fleet_sync_schema') ORDER BY name",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let receipts: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _fleet_sync_migrations ORDER BY version")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    let operation: String = sqlx::query_scalar(
        "SELECT operation_id || '|' || kind || '|' || request_key || '|' || state || '|' || finished_at FROM _fleet_sync_operations",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let step: CarriedStep = sqlx::query_as(
        "SELECT operation_id, position, step_name, kind, first_attempt_at, may_have_reached FROM _fleet_sync_steps",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    (tables, receipts, operation, step)
}

#[tokio::test]
async fn released_journals_convert_on_open_keep_their_rows_and_reopen() {
    for journal in [
        PrecedingJournal::MarkerV1,
        PrecedingJournal::ExtendedReceipts,
    ] {
        // Arrange: the predecessor's file, as its Router left it.
        let (_directory, path) = predecessor(journal).await;

        // Act: the public opener, twice.
        for _ in 0..2 {
            let owner = InteractionStore::open(&path).await.unwrap();
            let mut connection = owner.writer.lock().await;
            let (tables, receipts, operation, step) = journal_state(&mut connection).await;

            // Assert: the converted journal, its receipts, and the carried rows unchanged.
            assert_eq!(
                tables,
                vec![
                    "_fleet_sync_migrations",
                    "_fleet_sync_operations",
                    "_fleet_sync_steps"
                ]
            );
            assert_eq!(receipts, vec![1, 2, 3]);
            assert_eq!(
                operation,
                format!("{OPERATION}|fixture.kind|request-1|completed|1794000005000")
            );
            assert_eq!(
                step,
                (
                    OPERATION.to_owned(),
                    0,
                    "write".to_owned(),
                    "effect".to_owned(),
                    Some(1794000004000),
                    Some(1)
                )
            );
            drop(connection);
            owner.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn a_tampered_or_half_converted_journal_is_refused_without_rewriting_the_file() {
    for (journal, statement) in [
        // Half of journal 0002 applied to a 0.1.125 file.
        (
            PrecedingJournal::MarkerV1,
            "ALTER TABLE _fleet_sync_operations ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        ),
        // A released marker left behind beside converted receipts.
        (
            PrecedingJournal::Pinned,
            "CREATE TABLE _fleet_sync_schema (version INTEGER PRIMARY KEY NOT NULL)",
        ),
        // A converted receipt table with an extra column.
        (
            PrecedingJournal::Pinned,
            "ALTER TABLE _fleet_sync_migrations ADD COLUMN extra TEXT",
        ),
        // A converted journal table with a column no library writes; the
        // library itself accepts it, so only admission can refuse it.
        (
            PrecedingJournal::Pinned,
            "ALTER TABLE _fleet_sync_operations ADD COLUMN extra TEXT",
        ),
        // Extended receipts without the journal metadata they came with.
        (
            PrecedingJournal::ExtendedReceipts,
            "DROP TABLE _fleet_sync_schema",
        ),
    ] {
        // Arrange
        let (_directory, path) = predecessor(journal).await;
        let mut connection = writer(&path).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        let before = tokio::fs::read(&path).await.unwrap();

        // Act
        let opened = InteractionStore::open(&path).await;

        // Assert
        assert!(
            matches!(opened, Err(crate::StorageError::InvalidSchema)),
            "accepted a malformed journal: {statement}"
        );
        assert_eq!(
            tokio::fs::read(&path).await.unwrap(),
            before,
            "refusal rewrote the file: {statement}"
        );
    }
}
