//! Native migration prefixes retain data and agree with ordinary SQLite catalogs.

use crate::{
    PROJECT_MIGRATOR, ProjectStore, StoreSchemaState, schema_fingerprint::schema_fingerprint,
};
use message_board::{FeedContent, FeedCursor, FeedEntry, FeedPosition, ProjectId, RecordPosition};
use sqlx::{ConnectOptions, Connection, migrate::Migrator};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn every_migration_prefix_matches_sqlite_without_cdc_engine_tables() -> TestResult {
    let mut native = sqlx_turso::TursoConnectOptions::new().connect().await?;
    sqlx::query("PRAGMA capture_data_changes_conn('full')")
        .execute(&mut native)
        .await?;
    let mut sqlite = sqlx::sqlite::SqliteConnectOptions::new().connect().await?;
    assert_eq!(
        schema_fingerprint(&mut native).await?,
        sqlite_fingerprint(&mut sqlite).await?
    );
    for migration in PROJECT_MIGRATOR.iter() {
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut native)
            .await?;
        let prefix = Migrator::with_migrations(
            PROJECT_MIGRATOR
                .iter()
                .filter(|step| step.version <= migration.version)
                .cloned()
                .collect(),
        );
        let mut transaction = native.begin_with("BEGIN IMMEDIATE").await?;
        prefix.run_direct(None, &mut *transaction, false).await?;
        transaction.commit().await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut native)
            .await?;
        prefix.run(&mut sqlite).await?;
        let holder = schema_fingerprint(&mut native).await?;
        let copy = sqlite_fingerprint(&mut sqlite).await?;
        if holder != copy {
            let native_entries: Vec<(String, String, Option<String>)> =
                sqlx::query_as("SELECT type, name, sql FROM sqlite_schema ORDER BY type,name")
                    .fetch_all(&mut native)
                    .await?;
            let sqlite_entries: Vec<(String, String, Option<String>)> =
                sqlx::query_as("SELECT type, name, sql FROM sqlite_schema ORDER BY type,name")
                    .fetch_all(&mut sqlite)
                    .await?;
            for native in native_entries {
                if let Some(copy) = sqlite_entries
                    .iter()
                    .find(|entry| entry.0 == native.0 && entry.1 == native.1)
                {
                    let normalize = |entry: &Option<String>| {
                        entry
                            .as_deref()
                            .unwrap_or_default()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    if normalize(&native.2) != normalize(&copy.2) {
                        eprintln!(
                            "catalog difference {}:\n native: {:?}\n sqlite: {:?}",
                            native.1, native.2, copy.2
                        );
                    }
                } else {
                    eprintln!("native-only catalog entry: {native:?}");
                }
            }
        }
        assert_eq!(holder, copy, "migration {}", migration.version);
    }
    Ok(())
}

async fn sqlite_fingerprint(
    connection: &mut sqlx::SqliteConnection,
) -> Result<message_board::SchemaFingerprint, Box<dyn std::error::Error + Send + Sync>> {
    let entries: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT type, name, sql FROM sqlite_schema")
            .fetch_all(connection)
            .await?;
    Ok(crate::schema_fingerprint::fingerprint_entries(
        entries
            .into_iter()
            .map(|(kind, name, sql)| (kind, name, sql.unwrap_or_default()))
            .collect(),
    )?)
}

#[tokio::test]
async fn opening_a_populated_previous_version_migrates_without_changing_its_feed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("previous.sqlite");
    let project_id = ProjectId::generate();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .connect()
        .await?;
    sqlx::query("PRAGMA capture_data_changes_conn('full')")
        .execute(&mut connection)
        .await?;
    let first = PROJECT_MIGRATOR
        .iter()
        .next()
        .ok_or("missing first migration")?;
    let prefix = Migrator::with_migrations(vec![first.clone()]);
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut connection)
        .await?;
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    prefix.run_direct(None, &mut *transaction, false).await?;
    let cursor = FeedCursor {
        epoch: message_board::EpochNumber::try_from(1)?,
        position: FeedPosition::try_from(1)?,
    };
    let mut record = crate::project_store_tests::project_record(project_id.clone());
    record.envelope_mut().position = RecordPosition::Project(cursor);
    let payload = serde_json::to_string(&record)?;
    let envelope = record.envelope();
    let entry = FeedEntry {
        epoch: cursor.epoch,
        position: cursor.position,
        record: record.reference(),
        request: envelope.request.clone(),
        router_time: envelope.router_time,
        actor: envelope.actor.clone(),
        content: FeedContent::Shown(Box::new(record.clone())),
    };
    let encoded = serde_json::to_string(&entry)?;
    sqlx::query("INSERT INTO record_identities(kind,id,payload) VALUES ('project',?,?)")
        .bind(project_id.as_str())
        .bind(&payload)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO project_state(project_id) VALUES (?)")
        .bind(project_id.as_str())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO project_feed(position,epoch,record_kind,record_id,entry) VALUES (1,1,'project',?,?)").bind(project_id.as_str()).bind(encoded).execute(&mut *transaction).await?;
    sqlx::query("UPDATE feed_checkpoint SET last_position=1 WHERE singleton=1")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    connection.close().await?;
    let store = ProjectStore::open(&path, project_id.clone()).await?;
    assert!(matches!(
        store.schema_state(),
        StoreSchemaState::Ready {
            version: crate::project_schema::PROJECT_SCHEMA_VERSION,
            ..
        }
    ));
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), cursor);
    assert_eq!(read.record(&project_id.into()).await?, Some(record));
    let page = read
        .feed_after(
            FeedCursor {
                position: FeedPosition::ZERO,
                ..cursor
            },
            10,
            false,
        )
        .await?;
    assert!(
        matches!(page, message_board::FeedRead::Entries { entries, covered_through, .. } if entries == vec![entry] && covered_through == cursor)
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}
