use automation_storage::{AutomationStore, StorageError};
use sqlx::{Connection, Row, SqliteConnection};
use std::path::{Path, PathBuf};

const LEGACY_V1_SCHEMA: &str = include_str!("fixtures/automation_v1.sql");

macro_rules! ensure {
    ($condition:expr) => {
        if !$condition {
            return Err(format!("assertion failed: {}", stringify!($condition)).into());
        }
    };
    ($condition:expr, $context:expr) => {
        if !$condition {
            return Err(
                format!("assertion failed: {}: {}", stringify!($condition), $context).into(),
            );
        }
    };
}

macro_rules! ensure_eq {
    ($left:expr, $right:expr) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!("values differ: left={left:?}, right={right:?}").into());
        }
    }};
    ($left:expr, $right:expr, $context:expr) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!(
                "values differ: left={left:?}, right={right:?}: {}",
                $context
            )
            .into());
        }
    }};
}

#[path = "schema_initialization/fixtures.rs"]
mod fixtures;
use fixtures::{WakeFixture, assert_preserved_domain_state, seed_all_domain_tables};

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new(label: &str) -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "automation-schema-{label}-{}.sqlite",
                uuid::Uuid::now_v7()
            )),
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn connect(path: &Path) -> Result<SqliteConnection, sqlx::Error> {
    SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
}

async fn create_legacy_v1(path: &Path, schema: &str) -> Result<(), sqlx::Error> {
    let mut connection = connect(path).await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(schema))
        .execute(&mut connection)
        .await?;
    sqlx::query("PRAGMA user_version=1")
        .execute(&mut connection)
        .await?;
    connection.close().await
}

async fn open_error(
    path: &Path,
    expectation: &str,
) -> Result<StorageError, Box<dyn std::error::Error>> {
    match AutomationStore::open(path).await {
        Err(error) => Ok(error),
        Ok(store) => {
            store.close().await?;
            Err(expectation.into())
        }
    }
}

async fn migration_history(path: &Path) -> Result<Vec<(i64, bool, Vec<u8>)>, sqlx::Error> {
    let mut connection = connect(path).await?;
    let rows =
        sqlx::query("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut connection)
            .await?
            .into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect();
    connection.close().await?;
    Ok(rows)
}

async fn has_migration_history_table(path: &Path) -> Result<bool, sqlx::Error> {
    let mut connection = connect(path).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations')",
    )
    .fetch_one(&mut connection)
    .await?;
    connection.close().await?;
    Ok(exists)
}

#[tokio::test]
async fn fresh_database_runs_native_baseline_and_reopens() -> Result<(), Box<dyn std::error::Error>>
{
    let database = TestDatabase::new("fresh");
    AutomationStore::open(&database.path).await?.close().await?;
    let history = migration_history(&database.path).await?;
    ensure_eq!(history.len(), 15);
    // Central receipt custody and the local-only wake slot extend the same ledger.
    ensure_eq!(history.last().map(|row| row.0), Some(20261031000200));
    ensure!(history[0].1);
    ensure!(!history[0].2.is_empty());
    AutomationStore::open(&database.path).await?.close().await?;
    ensure_eq!(migration_history(&database.path).await?, history);
    Ok(())
}

#[tokio::test]
async fn exact_v1_is_adopted_without_changing_domain_state()
-> Result<(), Box<dyn std::error::Error>> {
    let database = TestDatabase::new("adopt-v1");
    create_legacy_v1(&database.path, LEGACY_V1_SCHEMA).await?;
    seed_all_domain_tables(&database.path, WakeFixture::Valid).await?;
    AutomationStore::open(&database.path).await?.close().await?;
    assert_preserved_domain_state(&database.path, WakeFixture::Valid).await?;
    let history = migration_history(&database.path).await?;
    ensure_eq!(history.len(), 15);
    // Central receipt custody and the local-only wake slot extend the same ledger.
    ensure_eq!(history.last().map(|row| row.0), Some(20261031000200));
    AutomationStore::open(&database.path).await?.close().await?;
    assert_preserved_domain_state(&database.path, WakeFixture::Valid).await?;
    ensure_eq!(migration_history(&database.path).await?, history);
    Ok(())
}

#[tokio::test]
async fn invalid_legacy_wake_fails_closed_with_field_tag_without_partial_adoption()
-> Result<(), Box<dyn std::error::Error>> {
    // MS-10 upgrade: retain the original opaque/invalid all-table fixture as a
    // refusal case, including every pre-existing state-preservation assertion.
    for (fixture, field) in [
        (WakeFixture::Opaque, "wakeupId"),
        (WakeFixture::OpaqueDefinition, "definition"),
    ] {
        let database = TestDatabase::new("invalid-legacy-wake");
        create_legacy_v1(&database.path, LEGACY_V1_SCHEMA).await?;
        seed_all_domain_tables(&database.path, fixture).await?;
        let error = open_error(&database.path, "invalid wake must not be adopted").await?;
        ensure!(
            matches!(error, StorageError::InvalidWake { field: actual, .. } if actual == field)
        );
        assert_preserved_domain_state(&database.path, fixture).await?;
        ensure!(!has_migration_history_table(&database.path).await?);
    }
    Ok(())
}

#[tokio::test]
async fn receipt_column_migration_preserves_existing_rows() -> Result<(), Box<dyn std::error::Error>>
{
    let database = TestDatabase::new("receipt-rename");
    create_legacy_v1(&database.path, LEGACY_V1_SCHEMA).await?;
    seed_all_domain_tables(&database.path, WakeFixture::Valid).await?;
    let mut connection = connect(&database.path).await?;
    sqlx::query(
        "UPDATE mailbox_deliveries SET accepted_receipt_json=? WHERE delivery_id='delivery-1'",
    )
    .bind("{\"legacy\":true}")
    .execute(&mut connection)
    .await?;
    connection.close().await?;

    AutomationStore::open(&database.path).await?.close().await?;
    let mut connection = connect(&database.path).await?;
    let retained: String = sqlx::query_scalar(
        "SELECT outcome_receipt_json FROM mailbox_deliveries WHERE delivery_id='delivery-1'",
    )
    .fetch_one(&mut connection)
    .await?;
    ensure_eq!(retained, "{\"legacy\":true}");
    connection.close().await?;
    ensure_eq!(migration_history(&database.path).await?.len(), 15);

    let valid_delivery_id = agent_automation::DeliveryId::generate();
    let mut connection = connect(&database.path).await?;
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut connection)
        .await?;
    sqlx::query("UPDATE mailbox_deliveries SET delivery_id=?,latest_attempt_json=NULL,outcome_receipt_json='not-json' WHERE delivery_id='delivery-1'")
        .bind(valid_delivery_id.as_str())
        .execute(&mut connection).await?;
    sqlx::query("UPDATE wakeup_definitions SET pending_delivery_id=? WHERE pending_delivery_id='delivery-1'")
        .bind(valid_delivery_id.as_str())
        .execute(&mut connection).await?;
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let mut store = AutomationStore::open(&database.path).await?;
    let result = store
        .read_delivery::<serde_json::Value, serde_json::Value, serde_json::Value>(
            &valid_delivery_id,
        )
        .await;
    ensure!(matches!(result, Err(StorageError::InvalidRecord)));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn incompatible_v1_shapes_are_rejected_without_adoption()
-> Result<(), Box<dyn std::error::Error>> {
    let variants = [
        (
            "column-type",
            LEGACY_V1_SCHEMA.replacen(
                "instruction_text TEXT NOT NULL",
                "instruction_text BLOB NOT NULL",
                1,
            ),
        ),
        (
            "column-nullability",
            LEGACY_V1_SCHEMA.replacen("updated_at_ms INTEGER NOT NULL", "updated_at_ms INTEGER", 1),
        ),
        (
            "column-default",
            LEGACY_V1_SCHEMA.replacen(
                "updated_at_ms INTEGER NOT NULL",
                "updated_at_ms INTEGER NOT NULL DEFAULT 7",
                1,
            ),
        ),
        (
            "enabled-check",
            LEGACY_V1_SCHEMA.replacen(
                "enabled INTEGER NOT NULL CHECK (enabled IN (0,1))",
                "enabled INTEGER NOT NULL",
                1,
            ),
        ),
        (
            "extra-check",
            LEGACY_V1_SCHEMA.replacen(
                "operation_status TEXT NOT NULL,",
                "operation_status TEXT NOT NULL CHECK(length(operation_status)>0),",
                1,
            ),
        ),
        (
            "foreign-key-target",
            LEGACY_V1_SCHEMA.replacen(
                "REFERENCES schedule_definitions(schedule_id)",
                "REFERENCES instruction_documents(instruction_id)",
                1,
            ),
        ),
        (
            "foreign-key-deferral",
            LEGACY_V1_SCHEMA.replacen(" DEFERRABLE INITIALLY DEFERRED", "", 1),
        ),
        (
            "required-index",
            LEGACY_V1_SCHEMA.replacen(
                "CREATE INDEX event_cleanup ON automation_events(recorded_at_ms, event_sequence);",
                "CREATE INDEX event_cleanup ON automation_events(event_sequence, recorded_at_ms);",
                1,
            ),
        ),
        (
            "index-sort-order",
            LEGACY_V1_SCHEMA.replacen(
                "CREATE INDEX event_cleanup ON automation_events(recorded_at_ms, event_sequence);",
                "CREATE INDEX event_cleanup ON automation_events(recorded_at_ms DESC, event_sequence);",
                1,
            ),
        ),
        (
            "index-collation",
            LEGACY_V1_SCHEMA.replacen(
                "CREATE INDEX event_cleanup ON automation_events(recorded_at_ms, event_sequence);",
                "CREATE INDEX event_cleanup ON automation_events(recorded_at_ms COLLATE NOCASE, event_sequence);",
                1,
            ),
        ),
        (
            "strict-table",
            LEGACY_V1_SCHEMA.replacen(
                " committed_at_ms INTEGER NOT NULL\n);",
                " committed_at_ms INTEGER NOT NULL\n) STRICT;",
                1,
            ),
        ),
        (
            "without-rowid",
            LEGACY_V1_SCHEMA.replacen(
                " next_due_at_ms INTEGER\n);",
                " next_due_at_ms INTEGER\n) WITHOUT ROWID;",
                1,
            ),
        ),
        (
            "missing-domain-table",
            LEGACY_V1_SCHEMA.replacen(
                "CREATE TABLE schedule_timing_state",
                "CREATE TABLE unexpected_timing_state",
                1,
            ),
        ),
        (
            "extra-domain-table",
            format!(
                "{LEGACY_V1_SCHEMA}\nCREATE TABLE unexpected_domain_table(id TEXT PRIMARY KEY);"
            ),
        ),
        (
            "unexpected-trigger",
            format!(
                "{LEGACY_V1_SCHEMA}\nCREATE TRIGGER unexpected_trigger AFTER INSERT ON automation_events BEGIN SELECT 1; END;"
            ),
        ),
    ];

    for (label, schema) in variants {
        let database = TestDatabase::new(label);
        create_legacy_v1(&database.path, &schema).await?;
        let mut connection = connect(&database.path).await?;
        sqlx::query("INSERT INTO automation_events(event_sequence,event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (41,'sentinel','run','run-1','kept','{\"kept\":true}',1)").execute(&mut connection).await?;
        connection.close().await?;

        let error = open_error(&database.path, "incompatible v1 schema must be rejected").await?;
        ensure!(
            matches!(error, StorageError::InvalidSchema),
            "{label}: {error}"
        );
        ensure!(
            !has_migration_history_table(&database.path).await?,
            "{label}"
        );
        let mut connection = connect(&database.path).await?;
        let sentinel: (i64, String) = sqlx::query_as("SELECT event_sequence,event_body_json FROM automation_events WHERE event_id='sentinel'").fetch_one(&mut connection).await?;
        ensure_eq!(sentinel, (41, "{\"kept\":true}".into()), "{label}");
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&mut connection)
            .await?;
        ensure_eq!(version, 1, "{label}");
        let high_water: i64 =
            sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='automation_events'")
                .fetch_one(&mut connection)
                .await?;
        ensure_eq!(high_water, 41, "{label}");
        connection.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn unknown_legacy_version_remains_unsupported() -> Result<(), Box<dyn std::error::Error>> {
    let database = TestDatabase::new("unsupported");
    create_legacy_v1(&database.path, LEGACY_V1_SCHEMA).await?;
    let mut connection = connect(&database.path).await?;
    sqlx::query("PRAGMA user_version=2")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let error = open_error(&database.path, "future legacy version must be rejected").await?;
    ensure!(matches!(error, StorageError::UnsupportedVersion(2)));
    ensure!(!has_migration_history_table(&database.path).await?);
    Ok(())
}

#[tokio::test]
async fn conflicting_native_history_is_rejected_without_domain_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let database = TestDatabase::new("history-mismatch");
    AutomationStore::open(&database.path).await?.close().await?;
    let mut connection = connect(&database.path).await?;
    sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES ('sentinel','run','run-1','kept','{}',1)").execute(&mut connection).await?;
    sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00'")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    let error = open_error(&database.path, "changed native migration must be rejected").await?;
    ensure!(matches!(error, StorageError::InvalidSchema));
    let mut connection = connect(&database.path).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM automation_events WHERE event_id='sentinel'")
            .fetch_one(&mut connection)
            .await?;
    ensure_eq!(count, 1);
    let checksum: Vec<u8> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations")
        .fetch_one(&mut connection)
        .await?;
    ensure_eq!(checksum, vec![0]);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn dirty_native_history_is_rejected_without_repair() -> Result<(), Box<dyn std::error::Error>>
{
    let database = TestDatabase::new("history-dirty");
    AutomationStore::open(&database.path).await?.close().await?;
    let mut connection = connect(&database.path).await?;
    sqlx::query("UPDATE _sqlx_migrations SET success=FALSE")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    let error = open_error(&database.path, "dirty native history must be rejected").await?;
    ensure!(matches!(error, StorageError::InvalidSchema));
    let mut connection = connect(&database.path).await?;
    let success: bool = sqlx::query_scalar("SELECT success FROM _sqlx_migrations")
        .fetch_one(&mut connection)
        .await?;
    ensure!(!success);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn held_writer_returns_database_error_without_partial_history_then_retry_succeeds()
-> Result<(), Box<dyn std::error::Error>> {
    let database = TestDatabase::new("contention");
    let mut lock = connect(&database.path).await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut lock).await?;
    let error = open_error(&database.path, "held writer must prevent initialization").await?;
    ensure!(matches!(error, StorageError::Database(_)));
    sqlx::query("ROLLBACK").execute(&mut lock).await?;
    lock.close().await?;
    ensure!(!has_migration_history_table(&database.path).await?);
    AutomationStore::open(&database.path).await?.close().await?;
    ensure_eq!(migration_history(&database.path).await?.len(), 15);
    Ok(())
}

#[tokio::test]
async fn concurrent_fresh_openers_converge_on_one_history_row()
-> Result<(), Box<dyn std::error::Error>> {
    let database = TestDatabase::new("concurrent");
    let (first, second) = tokio::join!(
        AutomationStore::open(&database.path),
        AutomationStore::open(&database.path)
    );
    first?.close().await?;
    second?.close().await?;
    ensure_eq!(migration_history(&database.path).await?.len(), 15);
    Ok(())
}
