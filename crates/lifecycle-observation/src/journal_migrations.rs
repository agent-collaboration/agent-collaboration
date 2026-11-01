//! Native migration ownership for the journal; legacy adoption never rewrites domain rows.
use collaboration_protocol::{AddressEntry, UuidIdentity};
use sqlx::{Connection, Row, SqliteConnection, migrate::MigrateError};

use crate::{JournalError, journal_retention::observation_timestamp_millis};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
const BASELINE_VERSION: i64 = 202_609_110_001;
const CURRENT_SCHEMA_SQL: &str = include_str!("journal_schema.sql");
const BASELINE_SQL: &str = include_str!("../migrations/202609110001_journal_baseline.sql");

pub(crate) async fn initialize(
    connection: &mut SqliteConnection,
    new_identity: UuidIdentity,
) -> Result<UuidIdentity, JournalError> {
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let journal_id = initialize_transaction(
        &mut transaction,
        new_identity,
        &MIGRATOR,
        CURRENT_SCHEMA_SQL,
    )
    .await?;
    transaction.commit().await?;
    Ok(journal_id)
}

async fn initialize_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    new_identity: UuidIdentity,
    migrator: &sqlx::migrate::Migrator,
    current_schema: &str,
) -> Result<UuidIdentity, JournalError> {
    let has_history: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    let has_domain_objects: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    let needs_checkpoint_backfill = !checkpoint_has_last_seen_column(transaction).await?;
    if !has_history && has_domain_objects {
        validate_schema(transaction, BASELINE_SQL).await?;
        read_identity(transaction).await?;
        migrator
            .run_direct(Some(BASELINE_VERSION), &mut **transaction, true)
            .await
            .map_err(map_migration_error)?;
    }
    migrator
        .run_direct(None, &mut **transaction, false)
        .await
        .map_err(map_migration_error)?;
    validate_schema(transaction, current_schema).await?;
    if !has_domain_objects {
        sqlx::query("INSERT INTO journal_checkpoint VALUES (1,0)")
            .execute(&mut **transaction)
            .await?;
        sqlx::query("INSERT INTO journal_metadata VALUES (1,1,?,0,0,0)")
            .bind(String::from(new_identity))
            .execute(&mut **transaction)
            .await?;
    }
    if needs_checkpoint_backfill {
        backfill_checkpoint_last_seen(transaction).await?;
    }
    read_identity(transaction).await
}

async fn checkpoint_has_last_seen_column(
    connection: &mut SqliteConnection,
) -> Result<bool, JournalError> {
    let has_table: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='checkpoint_addresses')",
    )
    .fetch_one(&mut *connection)
    .await?;
    if !has_table {
        return Ok(false);
    }
    let columns = sqlx::query("PRAGMA table_info(checkpoint_addresses)")
        .fetch_all(connection)
        .await?;
    columns
        .into_iter()
        .map(|row| row.try_get::<String, _>("name"))
        .collect::<Result<Vec<_>, _>>()
        .map(|columns| columns.iter().any(|column| column == "last_seen_ms"))
        .map_err(JournalError::Storage)
}

async fn backfill_checkpoint_last_seen(
    connection: &mut SqliteConnection,
) -> Result<(), JournalError> {
    let journal_id = read_identity(connection).await?;
    let mut cursor = String::new();
    loop {
        let rows=sqlx::query("SELECT address_key,entry_json FROM checkpoint_addresses WHERE last_seen_ms=0 AND address_key>? ORDER BY address_key LIMIT 100")
            .bind(&cursor)
            .fetch_all(&mut *connection)
            .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let address_key: String = row.try_get("address_key")?;
            cursor.clone_from(&address_key);
            let encoded: String = row.try_get("entry_json")?;
            let invalid_row = || JournalError::InvalidCheckpointAddress {
                address_key: address_key.clone(),
            };
            let entry: AddressEntry = serde_json::from_str(&encoded).map_err(|_| invalid_row())?;
            if serde_json::to_string(&entry.address).map_err(|_| invalid_row())? != address_key
                || entry.last_observation.journal_id != journal_id
            {
                return Err(invalid_row());
            }
            let last_seen_ms =
                observation_timestamp_millis(&entry.last_observed_at).map_err(|_| invalid_row())?;
            if last_seen_ms == 0 {
                return Err(invalid_row());
            }
            let updated = sqlx::query(
                "UPDATE checkpoint_addresses SET last_seen_ms=? WHERE address_key=? AND last_seen_ms=0",
            )
            .bind(last_seen_ms)
            .bind(address_key)
            .execute(&mut *connection)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(JournalError::InvalidStorage);
            }
        }
    }
    Ok(())
}

async fn read_identity(connection: &mut SqliteConnection) -> Result<UuidIdentity, JournalError> {
    let row = sqlx::query("SELECT version,journal_id FROM journal_metadata WHERE singleton=1")
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(JournalError::InvalidStorage)?;
    if row.try_get::<i64, _>("version")? != 1 {
        return Err(JournalError::InvalidStorage);
    }
    let checkpoint: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM journal_checkpoint WHERE singleton=1)")
            .fetch_one(connection)
            .await?;
    if !checkpoint {
        return Err(JournalError::InvalidStorage);
    }
    UuidIdentity::try_from(row.try_get::<String, _>("journal_id")?)
        .map_err(|_| JournalError::InvalidStorage)
}

// The legacy initializer emitted these exact definitions. Compare all objects,
// including constraints, so a version marker cannot bless an incompatible database.
// Future migrations must update the current target definition source here too.
async fn validate_schema(
    connection: &mut SqliteConnection,
    expected_schema: &str,
) -> Result<(), JournalError> {
    let rows = sqlx::query(
        "SELECT sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(connection)
    .await?;
    let mut actual = rows
        .into_iter()
        .map(|row| {
            row.try_get::<String, _>("sql")
                .map(|sql| normalize_definition(&sql))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected = expected_schema
        .split(';')
        .filter(|statement| !statement.trim().is_empty())
        .map(normalize_definition)
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual != expected {
        return Err(JournalError::InvalidStorage);
    }
    Ok(())
}

fn normalize_definition(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn map_migration_error(error: MigrateError) -> JournalError {
    match error {
        MigrateError::Execute(error) | MigrateError::ExecuteMigration(error, _) => {
            JournalError::Storage(error)
        }
        _ => JournalError::InvalidStorage,
    }
}

#[cfg(test)]
mod tests;
