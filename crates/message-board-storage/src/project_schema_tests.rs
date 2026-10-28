//! Native migration prefixes retain data and agree with ordinary SQLite catalogs.

use crate::{PROJECT_MIGRATOR, schema_fingerprint::schema_fingerprint};
use sqlx::{ConnectOptions, Connection, migrate::Migrator};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
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
