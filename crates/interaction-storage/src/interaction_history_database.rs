//! Owned interaction database initialization, checked rows and row-delta commits.
use super::interaction_history_codec::decode_stored_record;
use super::{InteractionHistoryData, InteractionHistoryError};
use chrono::{DateTime, Utc};
use sqlx::{Connection, Transaction};
use sqlx_turso::{Turso, TursoConnectOptions, TursoConnection};
use std::{
    ops::{Deref, DerefMut},
    path::Path,
    time::Duration,
};

#[path = "interaction_history_schema.rs"]
mod interaction_history_schema;
use interaction_history_schema::{SchemaAdmission, inspect_schema};

pub(super) static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub(super) use crate::StorageError;

impl StorageError {
    pub(super) fn public_error(self, startup: bool) -> InteractionHistoryError {
        if startup {
            tracing::error!(reason = ?self, "interaction_history_startup_failed");
        } else {
            tracing::error!(reason = ?self, "interaction_history_mutation_failed");
        }
        InteractionHistoryError::Unavailable
    }
}

pub(crate) struct HistoryDatabaseState {
    pub(super) cache: InteractionHistoryData,
    pub(super) revision: i64,
}

impl Deref for HistoryDatabaseState {
    type Target = InteractionHistoryData;
    fn deref(&self) -> &Self::Target {
        &self.cache
    }
}
impl DerefMut for HistoryDatabaseState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cache
    }
}

struct StoredInteractionRow {
    request_id: String,
    record_json: String,
    created_at: String,
    request_storage: String,
    record_storage: String,
    timestamp_storage: String,
}

struct HistoryRevisionRow {
    metadata_id: i64,
    pub(super) revision: i64,
    metadata_storage: String,
    revision_storage: String,
}

pub(super) async fn read_revision(connection: &mut TursoConnection) -> Result<i64, StorageError> {
    let mut rows = sqlx_turso::query_as!(HistoryRevisionRow,
        "SELECT metadata_id AS \"metadata_id!: i64\", revision AS \"revision!: i64\", typeof(metadata_id) AS \"metadata_storage!: String\", typeof(revision) AS \"revision_storage!: String\" FROM interaction_history_revision")
        .fetch_all(connection).await.map_err(|_| StorageError::InvalidSchema)?;
    if rows.len() != 1 {
        return Err(StorageError::InvalidSchema);
    }
    let row = rows.pop().ok_or(StorageError::InvalidSchema)?;
    if row.metadata_id != 1
        || row.revision < 0
        || row.metadata_storage != "integer"
        || row.revision_storage != "integer"
    {
        return Err(StorageError::InvalidSchema);
    }
    Ok(row.revision)
}

pub(super) async fn read_records(
    connection: &mut TursoConnection,
) -> Result<InteractionHistoryData, StorageError> {
    let rows = sqlx_turso::query_as!(StoredInteractionRow,
        "SELECT request_id AS \"request_id!: String\", record_json AS \"record_json!: String\", created_at AS \"created_at!: String\", typeof(request_id) AS \"request_storage!: String\", typeof(record_json) AS \"record_storage!: String\", typeof(created_at) AS \"timestamp_storage!: String\" FROM typed_interaction_history ORDER BY request_id")
        .fetch_all(connection).await.map_err(|_| StorageError::InvalidStoredRecord)?;
    let mut data = InteractionHistoryData::default();
    for row in rows {
        if row.request_storage != "text"
            || row.record_storage != "text"
            || row.timestamp_storage != "text"
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        let (record, created_at) =
            decode_stored_record(&row.request_id, &row.record_json, &row.created_at)?;
        data.insert_with_timestamp(row.request_id, record, created_at);
    }
    Ok(data)
}

fn format_timestamp(created_at: DateTime<Utc>) -> String {
    created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

pub(crate) async fn initialize_history(
    database_path: &Path,
) -> Result<(TursoConnection, HistoryDatabaseState), StorageError> {
    match tokio::fs::metadata(database_path).await {
        Ok(metadata) if metadata.is_file() => {
            let options = TursoConnectOptions::new()
                .filename(database_path)
                .read_only(true);
            let mut observer = TursoConnection::connect_with(&options)
                .await
                .map_err(|_| StorageError::InvalidSchema)?;
            inspect_schema(&mut observer).await?;
            observer
                .close()
                .await
                .map_err(|_| StorageError::StorageUnavailable)?;
        }
        Ok(_) => return Err(StorageError::InvalidSchema),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(StorageError::StorageUnavailable),
    }
    let options = TursoConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(1));
    let mut connection = TursoConnection::connect_with(&options)
        .await
        .map_err(|_| StorageError::StorageUnavailable)?;
    sqlx::query("PRAGMA synchronous=FULL")
        .execute(&mut connection)
        .await
        .map_err(|_| StorageError::StorageUnavailable)?;
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&mut connection)
        .await
        .map_err(|_| StorageError::StorageUnavailable)?;
    let mut transaction = connection
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|_| StorageError::StorageUnavailable)?;
    let initialized = async {
        let admission = inspect_schema(&mut transaction).await?;
        MIGRATOR
            .run_direct(None, &mut *transaction, false)
            .await
            .map_err(|_| StorageError::InvalidSchema)?;
        if admission == SchemaAdmission::Pristine {
            sqlx_turso::query!(
                "INSERT INTO interaction_history_revision (metadata_id, revision) VALUES (1, 0)"
            )
            .execute(&mut *transaction)
            .await
            .map_err(|_| StorageError::StorageUnavailable)?;
        }
        durable_steps::turso_store::migrate_journal(&mut transaction)
            .await
            .map_err(|_| StorageError::InvalidSchema)?;
        let mut delivery_migrator = sqlx::migrate::Migrator::with_migrations(
            fleet_sync_delivery::DELIVERY_MIGRATOR
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        );
        delivery_migrator.dangerous_set_table_name("_fleet_sync_delivery_migrations");
        delivery_migrator
            .run_direct(None, &mut *transaction, false)
            .await
            .map_err(|_| StorageError::InvalidSchema)?;
        let revision = read_revision(&mut transaction).await?;
        let cache = read_records(&mut transaction).await?;
        Ok::<_, StorageError>((cache, revision))
    }
    .await;
    let (cache, revision) = match initialized {
        Ok(state) => state,
        Err(error) => {
            transaction
                .rollback()
                .await
                .map_err(|_| StorageError::StorageUnavailable)?;
            return Err(error);
        }
    };
    transaction
        .commit()
        .await
        .map_err(|_| StorageError::StorageUnavailable)?;
    Ok((connection, HistoryDatabaseState { cache, revision }))
}

enum HistoryRowDelta<'record> {
    Insert {
        request_id: &'record str,
        record_json: String,
        created_at: String,
    },
    Update {
        request_id: &'record str,
        record_json: String,
    },
    Delete {
        request_id: &'record str,
    },
}

pub(super) async fn commit_delta(
    transaction: &mut Transaction<'_, Turso>,
    previous: &InteractionHistoryData,
    next: &InteractionHistoryData,
    revision: i64,
) -> Result<i64, StorageError> {
    if next.records.len() != next.created_at.len() {
        return Err(StorageError::InvalidStoredRecord);
    }
    let mut changes = Vec::new();
    for request_id in previous.records.keys() {
        if !next.records.contains_key(request_id) {
            changes.push(HistoryRowDelta::Delete { request_id });
        }
    }
    for (request_id, record) in &next.records {
        if request_id != record.request_id() || !record.is_valid_stored_value() {
            return Err(StorageError::InvalidStoredRecord);
        }
        let created_at = next
            .created_at
            .get(request_id)
            .ok_or(StorageError::InvalidStoredRecord)?;
        let record_json =
            serde_json::to_string(record).map_err(|_| StorageError::InvalidStoredRecord)?;
        match previous.records.get(request_id) {
            Some(previous_record) => {
                if previous.created_at.get(request_id) != Some(created_at) {
                    return Err(StorageError::InvalidStoredRecord);
                }
                if serde_json::to_string(previous_record)
                    .map_err(|_| StorageError::InvalidStoredRecord)?
                    != record_json
                {
                    changes.push(HistoryRowDelta::Update {
                        request_id,
                        record_json,
                    });
                }
            }
            None => changes.push(HistoryRowDelta::Insert {
                request_id,
                record_json,
                created_at: format_timestamp(*created_at),
            }),
        }
    }
    if changes.is_empty() {
        return Ok(revision);
    }
    // Validate revision advancement and the entire staged delta before issuing writes.
    let next_revision = revision.checked_add(1).ok_or(StorageError::InvalidSchema)?;
    for change in changes {
        match change {
            HistoryRowDelta::Insert {
                request_id,
                record_json,
                created_at,
            } => {
                sqlx_turso::query!("INSERT INTO typed_interaction_history (request_id, record_json, created_at) VALUES (?, ?, ?)", request_id,record_json,created_at)
                    .execute(&mut **transaction).await.map_err(|_| StorageError::StorageUnavailable)?;
            }
            HistoryRowDelta::Update {
                request_id,
                record_json,
            } => {
                sqlx_turso::query!(
                    "UPDATE typed_interaction_history SET record_json = ? WHERE request_id = ?",
                    record_json,
                    request_id
                )
                .execute(&mut **transaction)
                .await
                .map_err(|_| StorageError::StorageUnavailable)?;
            }
            HistoryRowDelta::Delete { request_id } => {
                sqlx_turso::query!(
                    "DELETE FROM typed_interaction_history WHERE request_id = ?",
                    request_id
                )
                .execute(&mut **transaction)
                .await
                .map_err(|_| StorageError::StorageUnavailable)?;
            }
        }
    }
    let updated = sqlx_turso::query!(
        "UPDATE interaction_history_revision SET revision = ? WHERE metadata_id = 1 AND revision = ?",
        next_revision,
        revision
    )
    .execute(&mut **transaction)
    .await
    .map_err(|_| StorageError::StorageUnavailable)?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::InvalidSchema);
    }
    Ok(next_revision)
}
