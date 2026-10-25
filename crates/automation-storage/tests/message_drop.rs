//! The owning migration refuses unverified state and commits its drop with its ledger.
use automation_storage::{AutomationStore, StorageError, apply_message_drop_migration};
use sqlx::{
    Connection, Row, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::path::Path;

type TestError = Box<dyn std::error::Error + Send + Sync>;
const BEFORE_DROP: i64 = 20_261_019_000_200;
const DROP_VERSION: i64 = 20_261_021_000_100;
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const MOVED: [&str; 4] = [
    "router_pushes",
    "subscription_delivery_progress",
    "board_intent_progress",
    "board_intent_scan_cursors",
];
const DROPPED: [&str; 5] = [
    "router_pushes",
    "subscription_delivery_progress",
    "board_intent_progress",
    "board_intent_scan_cursors",
    "mail_items",
];

async fn connect(path: &Path) -> Result<SqliteConnection, sqlx::Error> {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full),
    )
    .await
}
async fn before_drop(path: &Path) -> Result<(), TestError> {
    let mut connection = connect(path).await?;
    MIGRATOR
        .run_direct(Some(BEFORE_DROP), &mut connection, false)
        .await?;
    sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES ('kept-event','run','kept-run','fixture','{\"bytes\":\"kept\"}',17)")
        .execute(&mut connection).await?;
    connection.close().await?;
    Ok(())
}
async fn insert_opaque_source_row(path: &Path, table: &str) -> Result<(), TestError> {
    // The gate must preserve opaque source bytes even when a development row is invalid.
    let statement = match table {
        "router_pushes" => {
            "INSERT INTO router_pushes(push_id,kind,origin_kind,target_service_id,target_endpoint_id,target_session_id,header_facts_json,body,delivery_state,created_at) VALUES ('opaque-push','directMessage','owner','fixture-service','fixture-endpoint','fixture-session','{}','preserve these bytes','pending','2026-10-01T00:00:00Z')"
        }
        "subscription_delivery_progress" => {
            "INSERT INTO subscription_delivery_progress VALUES ('opaque-project','opaque-membership',1,7,'{\"opaque\":\"selected window\"}')"
        }
        "board_intent_progress" => {
            "INSERT INTO board_intent_progress VALUES ('opaque-project','opaque-intent',1,9)"
        }
        "board_intent_scan_cursors" => {
            "INSERT INTO board_intent_scan_cursors VALUES ('opaque-project',1,11)"
        }
        "mail_items" => {
            "INSERT INTO mail_items(item_id,digest,direction,status_json,status_version,payload_bytes,created_at) VALUES ('pre-release-item','opaque-digest','local','{\"kind\":\"unknownStatus\"}',1,17,'2026-10-01T00:00:00Z')"
        }
        _ => return Err("unknown source table".into()),
    };
    let mut connection = connect(path).await?;
    sqlx::query(statement).execute(&mut connection).await?;
    connection.close().await?;
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    schema: Vec<(String, String, Option<String>)>,
    rows: Vec<(String, Vec<Vec<String>>)>,
}
async fn snapshot(path: &Path) -> Result<Snapshot, TestError> {
    let mut connection = connect(path).await?;
    let schema = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
    )
    .fetch_all(&mut connection)
    .await?;
    let mut rows = Vec::new();
    for (_, table, _) in schema.iter().filter(|(kind, _, _)| kind == "table") {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut connection)
                .await?;
        let quoted = columns
            .iter()
            .map(|column| format!("quote(\"{}\")", column.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(",");
        let statement = format!(
            "SELECT {quoted} FROM \"{}\" ORDER BY rowid",
            table.replace('"', "\"\"")
        );
        // Only schema identifiers are interpolated, each quoted with doubled quotes.
        let records = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .fetch_all(&mut connection)
            .await?
            .into_iter()
            .map(|row| {
                (0..columns.len())
                    .map(|index| row.try_get::<String, _>(index))
                    .collect()
            })
            .collect::<Result<Vec<Vec<String>>, sqlx::Error>>()?;
        rows.push((table.clone(), records));
    }
    connection.close().await?;
    Ok(Snapshot { schema, rows })
}
async fn assert_dropped(path: &Path) -> Result<(), TestError> {
    let mut connection = connect(path).await?;
    for table in DROPPED {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        )
        .bind(table)
        .fetch_one(&mut connection)
        .await?;
        assert!(!exists, "{table} survived the recorded drop");
    }
    let recorded: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version=? AND success=1")
            .bind(DROP_VERSION)
            .fetch_one(&mut connection)
            .await?;
    assert_eq!(recorded, 1);
    connection.close().await?;
    Ok(())
}
fn retained_rows(snapshot: Snapshot) -> Vec<(String, Vec<Vec<String>>)> {
    snapshot
        .rows
        .into_iter()
        .filter(|(name, _)| !DROPPED.contains(&name.as_str()) && name != "_sqlx_migrations")
        .collect()
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn normal_startup_refuses_each_populated_message_table_without_changing_rows_or_ledger()
-> Result<(), TestError> {
    for table in MOVED {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("automation.sqlite");
        before_drop(&path).await?;
        insert_opaque_source_row(&path, table).await?;
        let before = snapshot(&path).await?;
        let error = match AutomationStore::open(&path).await {
            Ok(store) => {
                store.close().await?;
                return Err(format!("startup discarded {table}").into());
            }
            Err(error) => error,
        };
        assert!(
            matches!(&error, StorageError::MessageUpgradeRequired { tables } if tables == &vec![table]),
            "{error:?}"
        );
        assert!(error.to_string().contains("codex-router-board-upgrade"));
        assert_eq!(snapshot(&path).await?, before);
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn pre_release_mail_refuses_startup_and_maintenance_and_keeps_every_row()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    before_drop(&path).await?;
    insert_opaque_source_row(&path, "mail_items").await?;
    let before = snapshot(&path).await?;
    let error = match AutomationStore::open(&path).await {
        Ok(store) => {
            store.close().await?;
            return Err("startup discarded pre-release mail".into());
        }
        Err(error) => error,
    };
    assert!(
        matches!(&error, StorageError::PreReleaseMailPresent { rows: 1 }),
        "{error:?}"
    );
    assert!(error.to_string().contains("mail_items"));
    assert!(error.to_string().contains("pre-release"));
    assert_eq!(snapshot(&path).await?, before);
    let error = apply_message_drop_migration(&path)
        .await
        .expect_err("maintenance must also preserve pre-release mail");
    assert!(matches!(
        error,
        StorageError::PreReleaseMailPresent { rows: 1 }
    ));
    assert_eq!(snapshot(&path).await?, before);
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn an_empty_store_migrates_and_reopens_without_a_second_ledger_entry() -> Result<(), TestError>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    AutomationStore::open(&path).await?.close().await?;
    assert_dropped(&path).await?;
    let first = snapshot(&path).await?;
    AutomationStore::open(&path).await?.close().await?;
    assert_eq!(snapshot(&path).await?, first);
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn verified_copy_entry_drops_once_and_preserves_all_retained_automation_rows()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    before_drop(&path).await?;
    for table in MOVED {
        insert_opaque_source_row(&path, table).await?;
    }
    let mut expected = retained_rows(snapshot(&path).await?);
    // The owning migrator also creates the B3r request table after the drop.
    // It must start empty; every previously retained table keeps its exact rows.
    expected.push(("remote_wake_requests".to_owned(), Vec::new()));
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    // The verified copy is a caller precondition here; D4 owns its actual two-file proof.
    apply_message_drop_migration(&path).await?;
    assert_dropped(&path).await?;
    assert_eq!(retained_rows(snapshot(&path).await?), expected);
    let first = snapshot(&path).await?;
    apply_message_drop_migration(&path).await?;
    assert_eq!(snapshot(&path).await?, first);
    AutomationStore::open(&path).await?.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn a_failed_drop_ledger_write_rolls_back_all_five_drops_and_every_source_row()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    before_drop(&path).await?;
    for table in MOVED {
        insert_opaque_source_row(&path, table).await?;
    }
    let mut connection = connect(&path).await?;
    sqlx::raw_sql("CREATE TRIGGER fail_message_drop_ledger BEFORE INSERT ON _sqlx_migrations WHEN NEW.version=20261021000100 BEGIN SELECT RAISE(ABORT,'fixture message-drop ledger failure'); END")
        .execute(&mut connection).await?;
    connection.close().await?;
    let before = snapshot(&path).await?;
    let error = apply_message_drop_migration(&path)
        .await
        .expect_err("ledger fault must abort the migration");
    assert!(matches!(&error, StorageError::Database(_)), "{error:?}");
    assert_eq!(snapshot(&path).await?, before);
    let mut connection = connect(&path).await?;
    sqlx::raw_sql("DROP TRIGGER fail_message_drop_ledger")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    apply_message_drop_migration(&path).await?;
    assert_dropped(&path).await?;
    Ok(())
}
