use std::error::Error;

use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use super::{
    PROVIDER_OPERATION_RETENTION_MONTHS, ProviderOperationStore,
    provider_operation_retention_cutoff_ms,
};

#[test]
fn provider_operation_retention_defaults_to_two_calendar_months_and_keeps_day_override()
-> Result<(), Box<dyn Error>> {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-31T12:30:00Z")?.timestamp_millis();
    let default_cutoff = provider_operation_retention_cutoff_ms(now, None)?;
    let expected_default =
        chrono::DateTime::parse_from_rfc3339("2026-08-31T12:30:00Z")?.timestamp_millis();
    if PROVIDER_OPERATION_RETENTION_MONTHS != 2 {
        return Err(format!(
            "provider operation retention months was {PROVIDER_OPERATION_RETENTION_MONTHS}; expected 2"
        )
        .into());
    }
    if default_cutoff != expected_default {
        return Err(format!(
            "default retention cutoff was {default_cutoff}; expected {expected_default}"
        )
        .into());
    }

    let override_days = std::num::NonZeroU32::new(7).ok_or("positive fixture")?;
    let override_cutoff = provider_operation_retention_cutoff_ms(now, Some(override_days))?;
    let expected_override =
        chrono::DateTime::parse_from_rfc3339("2026-10-24T12:30:00Z")?.timestamp_millis();
    if override_cutoff != expected_override {
        return Err(format!(
            "day override cutoff was {override_cutoff}; expected {expected_override}"
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn new_provider_operation_database_uses_incremental_auto_vacuum() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("provider-operations.sqlite");
    let mut store = ProviderOperationStore::open(&database_path).await?;

    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut store.connection)
        .await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut store.connection)
        .await?;
    if mode != 2 {
        return Err(format!("expected PRAGMA auto_vacuum mode 2, got {mode}").into());
    }
    if journal_mode != "wal" {
        return Err(
            format!("provider operation journal mode was {journal_mode}; expected wal").into(),
        );
    }
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn existing_delete_mode_provider_database_reopens_in_wal_mode() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("provider-operations.sqlite");
    ProviderOperationStore::open(&database_path)
        .await?
        .close()
        .await?;

    let mut legacy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database_path)
            .journal_mode(SqliteJournalMode::Delete),
    )
    .await?;
    let legacy_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut legacy)
        .await?;
    legacy.close().await?;
    if legacy_mode != "delete" {
        return Err(format!(
            "legacy provider operation journal mode was {legacy_mode}; expected delete"
        )
        .into());
    }

    let mut store = ProviderOperationStore::open(&database_path).await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut store.connection)
        .await?;
    if journal_mode != "wal" {
        return Err(format!(
            "reopened provider operation journal mode was {journal_mode}; expected wal"
        )
        .into());
    }
    store.close().await?;
    Ok(())
}
