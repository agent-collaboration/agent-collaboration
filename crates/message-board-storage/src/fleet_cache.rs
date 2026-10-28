//! Only fleet replies, snapshots and feed pages update this read projection.
use crate::{
    RouterLocalStorageError,
    fleet_record_codec::{decode_fleet_record, decode_fleet_reference, validate_fleet_record},
    project_record_io::record_kind_tag,
    router_local_database::RouterLocalDatabase,
};
use fleet_admission::FleetFeedEntry;
use message_board::{FleetPosition, Record, RecordRef};
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetRecordSnapshot {
    pub through: FleetPosition,
    pub records: Vec<Record>,
}

#[path = "fleet_cache_pages.rs"]
mod pages;
pub use pages::{FleetCacheSnapshot, FleetListFilter, StoredFleetRecord};
#[path = "fleet_discovery_pages.rs"]
mod discovery;
pub use discovery::FleetDiscoveryKey;
#[path = "fleet_cache_rebuild.rs"]
mod rebuild;
pub use rebuild::FleetCacheRebuild;

static CACHE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./fleet_cache_migrations");
pub struct FleetCache {
    database: RouterLocalDatabase,
}

impl FleetCache {
    /// Scope selection reads keys only; bodies are validated when a consumer needs them.
    pub async fn workspace_project_ids(
        &self,
        workspace: &message_board::WorkspaceId,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        let workspace = workspace.as_str();
        let rows = sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'project' AND json_extract(payload, '$.record.envelope.scope.workspace') = ? ORDER BY id"#, workspace).fetch_all(&self.database.pool).await?;
        rows.into_iter()
            .map(|id| {
                message_board::ProjectId::try_from(id)
                    .map_err(|_| RouterLocalStorageError::InvalidRecord)
            })
            .collect()
    }

    pub async fn board_project_ids(
        &self,
        board: &message_board::BoardId,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        let board = board.as_str();
        sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'project' AND json_extract(payload, '$.record.board') = ? ORDER BY id"#, board).fetch_all(&self.database.pool).await?.into_iter().map(|id| message_board::ProjectId::try_from(id).map_err(|_| RouterLocalStorageError::InvalidRecord)).collect()
    }
    pub async fn project_ids(
        &self,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'project' ORDER BY id"#).fetch_all(&self.database.pool).await?.into_iter().map(|id| message_board::ProjectId::try_from(id).map_err(|_| RouterLocalStorageError::InvalidRecord)).collect()
    }
    pub async fn workspace_page_keys(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<message_board::WorkspaceId>, RouterLocalStorageError> {
        let limit = i64::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'workspace' AND id > ? ORDER BY id LIMIT ?"#, after, limit).fetch_all(&self.database.pool).await?.into_iter().map(|id| message_board::WorkspaceId::try_from(id).map_err(|_| RouterLocalStorageError::InvalidRecord)).collect()
    }
    pub async fn checkpoint(
        &self,
    ) -> Result<message_board::FleetPosition, RouterLocalStorageError> {
        cache_checkpoint(&mut *self.database.pool.acquire().await?).await
    }
    pub async fn open(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open(path, &CACHE_MIGRATOR).await?,
        })
    }
    pub async fn replace_snapshot(
        &self,
        snapshot: &FleetRecordSnapshot,
    ) -> Result<(), RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            sqlx_turso::query!("DELETE FROM cached_fleet_records")
                .execute(&mut *transaction)
                .await?;
            for record in &snapshot.records {
                let Some(position) = record.envelope().position.fleet_position() else {
                    return Err(RouterLocalStorageError::InvalidRecord);
                };
                if position > snapshot.through {
                    return Err(RouterLocalStorageError::InvalidRecord);
                }
                put_cached_record(&mut transaction, record).await?;
            }
            let through = i64::try_from(snapshot.through.get())
                .map_err(|_| RouterLocalStorageError::InvalidRecord)?;
            sqlx_turso::query!(
                "UPDATE fleet_cache_checkpoint SET position = ? WHERE singleton = 1",
                through
            )
            .execute(&mut *transaction)
            .await?;
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
    pub async fn apply_feed(
        &self,
        after: FleetPosition,
        entries: &[FleetFeedEntry],
    ) -> Result<(), RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let current = cache_checkpoint(&mut transaction).await?;
            if current != after {
                return Err(RouterLocalStorageError::PositionGap);
            }
            let mut previous = after;
            for entry in entries {
                if previous
                    .successor()
                    .map_err(|_| RouterLocalStorageError::InvalidRecord)?
                    != entry.position
                    || entry.record.envelope().position.fleet_position() != Some(entry.position)
                {
                    return Err(RouterLocalStorageError::PositionGap);
                }
                put_cached_record(&mut transaction, &entry.record).await?;
                previous = entry.position;
            }
            let position = i64::try_from(previous.get())
                .map_err(|_| RouterLocalStorageError::InvalidRecord)?;
            sqlx_turso::query!(
                "UPDATE fleet_cache_checkpoint SET position = ? WHERE singleton = 1",
                position
            )
            .execute(&mut *transaction)
            .await?;
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
    pub async fn record(
        &self,
        reference: &RecordRef,
    ) -> Result<Option<Record>, RouterLocalStorageError> {
        let kind = record_kind_tag(reference.kind);
        let id = reference.id.as_str();
        let payload = sqlx_turso::query_scalar!(r#"SELECT payload AS "payload!: String" FROM cached_fleet_records WHERE kind = ? AND id = ?"#, kind, id).fetch_optional(&self.database.pool).await?;
        payload
            .map(|payload| decode_fleet_record(reference, &payload).map_err(record_decode_error))
            .transpose()
    }
    pub async fn snapshot(&self) -> Result<FleetRecordSnapshot, RouterLocalStorageError> {
        let mut transaction = self.database.pool.begin().await?;
        let through = cache_checkpoint(&mut transaction).await?;
        let rows = sqlx_turso::query!(r#"SELECT kind AS "kind!: String", id AS "id!: String", payload AS "payload!: String" FROM cached_fleet_records ORDER BY kind, id"#).fetch_all(&mut *transaction).await?;
        let records = rows
            .into_iter()
            .map(|row| {
                let reference =
                    decode_fleet_reference(&row.kind, row.id).map_err(record_decode_error)?;
                decode_fleet_record(&reference, &row.payload).map_err(record_decode_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await?;
        Ok(FleetRecordSnapshot { through, records })
    }
    pub async fn close(self) {
        self.database.close().await;
    }
}

fn record_decode_error(error: crate::FleetStorageError) -> RouterLocalStorageError {
    match error {
        crate::FleetStorageError::InvalidRecordAt { record } => {
            RouterLocalStorageError::InvalidRecordAt { record }
        }
        crate::FleetStorageError::InvalidRecordKey { field } => {
            RouterLocalStorageError::InvalidRecordKey { field }
        }
        _ => RouterLocalStorageError::InvalidRecord,
    }
}

async fn put_cached_record(
    connection: &mut sqlx_turso::TursoConnection,
    record: &Record,
) -> Result<(), RouterLocalStorageError> {
    validate_fleet_record(record).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    let reference = record.reference();
    let kind = record_kind_tag(reference.kind);
    let id = reference.id.as_str();
    let payload = serde_json::to_string(record)?;
    sqlx_turso::query!("INSERT INTO cached_fleet_records(kind, id, payload) VALUES (?, ?, ?) ON CONFLICT(kind, id) DO UPDATE SET payload = excluded.payload", kind, id, payload).execute(connection).await?;
    Ok(())
}
async fn cache_checkpoint(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<FleetPosition, RouterLocalStorageError> {
    let row = sqlx_turso::query!(r#"SELECT position AS "position!: i64", (SELECT COUNT(*) FROM fleet_cache_checkpoint) AS "count!: i64" FROM fleet_cache_checkpoint WHERE singleton = 1"#).fetch_one(connection).await?;
    if row.count != 1 {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    FleetPosition::try_from(
        u64::try_from(row.position).map_err(|_| RouterLocalStorageError::InvalidRecord)?,
    )
    .map_err(|_| RouterLocalStorageError::InvalidRecord)
}
