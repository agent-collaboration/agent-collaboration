//! Indexed directory keys and opaque body batches for bounded fleet responses.
use super::{FleetCache, record_decode_error};
use crate::{
    RouterLocalStorageError, fleet_record_codec::decode_fleet_record,
    project_record_io::record_kind_tag,
};
use message_board::{
    BoardId, ProjectId, Record, RecordKind, RecordRef, RepositoryRef, WorkspaceId,
};

pub enum FleetListFilter<'scope> {
    Workspaces {
        repository: Option<&'scope RepositoryRef>,
    },
    Boards {
        workspace: Option<&'scope WorkspaceId>,
        include_archived: bool,
    },
    Projects {
        board: &'scope BoardId,
    },
    Adrs {
        workspace: &'scope WorkspaceId,
    },
}
pub struct StoredFleetRecord {
    reference: RecordRef,
    payload: String,
}
impl StoredFleetRecord {
    pub fn reference(&self) -> &RecordRef {
        &self.reference
    }
    pub fn decode(self) -> Result<Record, RouterLocalStorageError> {
        decode_fleet_record(&self.reference, &self.payload).map_err(record_decode_error)
    }
}
pub struct FleetCacheSnapshot {
    pub(super) transaction: sqlx::Transaction<'static, sqlx_turso::Turso>,
}
impl FleetCache {
    pub async fn read_snapshot(&self) -> Result<FleetCacheSnapshot, RouterLocalStorageError> {
        Ok(FleetCacheSnapshot {
            transaction: self.database.pool.begin().await?,
        })
    }
}
impl FleetCacheSnapshot {
    pub async fn workspace_project_ids(
        &mut self,
        workspace: &WorkspaceId,
    ) -> Result<Vec<ProjectId>, RouterLocalStorageError> {
        let workspace = workspace.as_str();
        let ids = sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'project' AND json_extract(payload, '$.record.envelope.scope.workspace') = ? ORDER BY id"#, workspace).fetch_all(&mut *self.transaction).await?;
        ids.into_iter()
            .map(|id| ProjectId::try_from(id).map_err(|_| RouterLocalStorageError::InvalidRecord))
            .collect()
    }
    pub async fn finish(self) -> Result<(), RouterLocalStorageError> {
        Ok(self.transaction.commit().await?)
    }
    pub async fn list_page_keys(
        &mut self,
        filter: FleetListFilter<'_>,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, RouterLocalStorageError> {
        let limit = i64::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let (kind, ids) = match filter {
            FleetListFilter::Workspaces { repository: None } => (RecordKind::Workspace,
                sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'workspace' AND id > ? ORDER BY id LIMIT ?"#, after, limit).fetch_all(&mut *self.transaction).await?),
            FleetListFilter::Workspaces { repository: Some(repository) } => {
                let repository = serde_json::to_string(repository)?;
                (RecordKind::Workspace, sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'workspace' AND id > ?2 AND EXISTS (SELECT 1 FROM json_each(json_extract(payload, '$.record.repositories')) attached WHERE json_extract(attached.value, '$.kind') = json_extract(?1, '$.kind') AND ((json_extract(?1, '$.kind') = 'origin' AND json_extract(attached.value, '$.normalizedOrigin') = json_extract(?1, '$.normalizedOrigin')) OR (json_extract(?1, '$.kind') = 'local' AND json_extract(attached.value, '$.serviceId') = json_extract(?1, '$.serviceId') AND json_extract(attached.value, '$.commonDirectory') = json_extract(?1, '$.commonDirectory')))) ORDER BY id LIMIT ?3"#, repository, after, limit).fetch_all(&mut *self.transaction).await?)
            },
            FleetListFilter::Boards { workspace: Some(workspace), include_archived } => {
                let workspace = workspace.as_str();
                (RecordKind::Board, sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'board' AND json_extract(payload, '$.record.envelope.scope.workspace') = ? AND id > ? AND (? OR json_extract(payload, '$.record.state') = 'open') ORDER BY id LIMIT ?"#, workspace, after, include_archived, limit).fetch_all(&mut *self.transaction).await?)
            },
            FleetListFilter::Boards { workspace: None, include_archived } => (RecordKind::Board,
                sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'board' AND id > ? AND (? OR json_extract(payload, '$.record.state') = 'open') ORDER BY id LIMIT ?"#, after, include_archived, limit).fetch_all(&mut *self.transaction).await?),
            FleetListFilter::Projects { board } => {
                let board = board.as_str();
                (RecordKind::Project, sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'project' AND json_extract(payload, '$.record.board') = ? AND id > ? ORDER BY id LIMIT ?"#, board, after, limit).fetch_all(&mut *self.transaction).await?)
            },
            FleetListFilter::Adrs { workspace } => {
                let workspace = workspace.as_str();
                (RecordKind::Adr, sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM cached_fleet_records WHERE kind = 'adr' AND json_extract(payload, '$.record.envelope.scope.workspace') = ? AND id > ? ORDER BY id LIMIT ?"#, workspace, after, limit).fetch_all(&mut *self.transaction).await?)
            },
        };
        ids.into_iter()
            .map(|id| {
                crate::fleet_record_codec::decode_fleet_reference(record_kind_tag(kind), id)
                    .map_err(record_decode_error)
            })
            .collect()
    }
    pub async fn record_batch(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<StoredFleetRecord>, RouterLocalStorageError> {
        let keys = serde_json::to_string(
            &references
                .iter()
                .map(|key| (record_kind_tag(key.kind), key.id.as_str()))
                .collect::<Vec<_>>(),
        )?;
        let rows = sqlx_turso::query!(r#"SELECT r.kind AS "kind!: String", r.id AS "id!: String", r.payload AS "payload!: String" FROM json_each(?) k CROSS JOIN cached_fleet_records r ON r.kind = json_extract(k.value, '$[0]') AND r.id = json_extract(k.value, '$[1]') ORDER BY r.kind, r.id"#, keys).fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredFleetRecord {
                    reference: crate::fleet_record_codec::decode_fleet_reference(&row.kind, row.id)
                        .map_err(record_decode_error)?,
                    payload: row.payload,
                })
            })
            .collect()
    }
}
