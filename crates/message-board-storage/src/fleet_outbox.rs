//! Durable semantic requests survive the stand-in's eventual replacement by central.
use crate::{RouterLocalStorageError, router_local_database::RouterLocalDatabase};
use fleet_admission::{FleetIds, FleetRequest, canonical_hash};
use message_board::{RequestDecision, RequestId};
use serde::{Deserialize, Serialize};
use std::path::Path;

static OUTBOX_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./fleet_outbox_migrations");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FleetOutboxState {
    Pending {},
    Decided { decision: RequestDecision },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetOutboxEntry {
    pub request: FleetRequest,
    pub ids: FleetIds,
    pub state: FleetOutboxState,
}

pub struct FleetOutbox {
    database: RouterLocalDatabase,
}
impl FleetOutbox {
    pub async fn pending_request(
        &self,
        request: &RequestId,
    ) -> Result<Option<FleetOutboxEntry>, RouterLocalStorageError> {
        let id = request.as_str();
        let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
        sqlx_turso::query!(r#"SELECT request_id AS "request_id!: String", canonical_hash AS "canonical_hash!: String", request AS "request!: String", ids AS "ids!: String", state AS "state!: String" FROM fleet_outbox_requests WHERE request_id = ? AND state = ?"#, id, state)
            .fetch_optional(&self.database.pool).await?
            .map(|row| decode_entry(&row.request_id, &row.canonical_hash, &row.request, &row.ids, &row.state))
            .transpose()
    }
    pub async fn pending_project_ids(
        &self,
        workspace: &message_board::WorkspaceId,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        let workspace = workspace.as_str();
        let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
        let rows = sqlx_turso::query_scalar!(r#"SELECT json_extract(request, '$.operation.project') AS "project!: String" FROM fleet_outbox_requests WHERE state = ? AND json_extract(request, '$.workspace') = ? AND json_extract(request, '$.operation.kind') = 'projectDirectoryCreate' ORDER BY request_id"#, state, workspace).fetch_all(&self.database.pool).await?;
        rows.into_iter()
            .map(|id| {
                message_board::ProjectId::try_from(id)
                    .map_err(|_| RouterLocalStorageError::InvalidRecord)
            })
            .collect()
    }
    pub async fn pending_scoped_projects(
        &self,
        board: Option<&message_board::BoardId>,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
        let ids = match board {
            Some(board) => {
                let board = board.as_str();
                sqlx_turso::query_scalar!(r#"SELECT json_extract(request, '$.operation.project') AS "project!: String" FROM fleet_outbox_requests WHERE state = ? AND json_extract(request, '$.operation.kind') = 'projectDirectoryCreate' AND json_extract(request, '$.operation.board') = ? ORDER BY request_id"#, state, board).fetch_all(&self.database.pool).await?
            },
            None => sqlx_turso::query_scalar!(r#"SELECT json_extract(request, '$.operation.project') AS "project!: String" FROM fleet_outbox_requests WHERE state = ? AND json_extract(request, '$.operation.kind') = 'projectDirectoryCreate' ORDER BY request_id"#, state).fetch_all(&self.database.pool).await?,
        };
        ids.into_iter()
            .map(|id| {
                message_board::ProjectId::try_from(id)
                    .map_err(|_| RouterLocalStorageError::InvalidRecord)
            })
            .collect()
    }
    pub async fn open(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open(path, &OUTBOX_MIGRATOR).await?,
        })
    }
    pub async fn enqueue(
        &self,
        request: &FleetRequest,
        ids: &FleetIds,
    ) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = request.request_id.as_str();
        let result = async {
            let hash = canonical_hash(request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
            if let Some(row) = sqlx_turso::query!(r#"SELECT request AS "request!: String", ids AS "ids!: String", state AS "state!: String", canonical_hash AS "canonical_hash!: String" FROM fleet_outbox_requests WHERE request_id = ?"#, id).fetch_optional(&mut *transaction).await? {
                if row.canonical_hash != hash.as_str() { return Err(RouterLocalStorageError::ConflictingRequest); }
                return decode_entry(id, &row.canonical_hash, &row.request, &row.ids, &row.state);
            }
            let hash = hash.as_str(); let encoded = serde_json::to_string(request)?; let encoded_ids = serde_json::to_string(ids)?; let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
            sqlx_turso::query!("INSERT INTO fleet_outbox_requests(request_id, canonical_hash, request, ids, state) VALUES (?, ?, ?, ?, ?)", id, hash, encoded, encoded_ids, state).execute(&mut *transaction).await?;
            Ok(FleetOutboxEntry { request: request.clone(), ids: ids.clone(), state: FleetOutboxState::Pending {} })
        }.await;
        match result {
            Ok(entry) => {
                transaction.commit().await?;
                Ok(entry)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
    pub async fn pending(&self) -> Result<Vec<FleetOutboxEntry>, RouterLocalStorageError> {
        let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
        let rows = sqlx_turso::query!(r#"SELECT request_id AS "request_id!: String", canonical_hash AS "canonical_hash!: String", request AS "request!: String", ids AS "ids!: String", state AS "state!: String" FROM fleet_outbox_requests WHERE state = ? ORDER BY request_id"#, state).fetch_all(&self.database.pool).await?;
        rows.into_iter()
            .map(|row| {
                decode_entry(
                    &row.request_id,
                    &row.canonical_hash,
                    &row.request,
                    &row.ids,
                    &row.state,
                )
            })
            .collect()
    }
    pub async fn mark_decided(
        &self,
        request: &RequestId,
        decision: &RequestDecision,
    ) -> Result<(), RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = request.as_str();
        let state = sqlx_turso::query_scalar!(
            r#"SELECT state AS "state!: String" FROM fleet_outbox_requests WHERE request_id = ?"#,
            id
        )
        .fetch_one(&mut *transaction)
        .await?;
        let previous: FleetOutboxState =
            serde_json::from_str(&state).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        match previous {
            FleetOutboxState::Decided { decision: existing } if existing == *decision => {
                transaction.rollback().await?;
                Ok(())
            }
            FleetOutboxState::Decided { .. } => {
                transaction.rollback().await?;
                Err(RouterLocalStorageError::ConflictingRequest)
            }
            FleetOutboxState::Pending {} => {
                let state = serde_json::to_string(&FleetOutboxState::Decided {
                    decision: decision.clone(),
                })?;
                sqlx_turso::query!(
                    "UPDATE fleet_outbox_requests SET state = ? WHERE request_id = ?",
                    state,
                    id
                )
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                Ok(())
            }
        }
    }
    pub async fn close(self) {
        self.database.close().await;
    }
}

fn decode_entry(
    id: &str,
    hash: &str,
    request: &str,
    ids: &str,
    state: &str,
) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
    let request: FleetRequest =
        serde_json::from_str(request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    let canonical = canonical_hash(&request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    if request.request_id.as_str() != id || canonical.as_str() != hash {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    Ok(FleetOutboxEntry {
        request,
        ids: serde_json::from_str(ids).map_err(|_| RouterLocalStorageError::InvalidRecord)?,
        state: serde_json::from_str(state).map_err(|_| RouterLocalStorageError::InvalidRecord)?,
    })
}
