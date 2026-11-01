//! Durable semantic requests survive the stand-in's eventual replacement by central.
use crate::{RouterLocalStorageError, router_local_database::RouterLocalDatabase};
use fleet_admission::{FleetRequest, canonical_hash};
use message_board::{RequestDecision, RequestId};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::Path;

mod creation_input;
#[cfg(test)]
mod creation_tests;

static OUTBOX_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./fleet_outbox_migrations");
const MAX_FLEET_OUTBOX_PRUNE_BATCH: u32 = 500;

/// Retains completed fleet outbox requests and inputs for two calendar months.
pub(crate) const FLEET_OUTBOX_RETENTION_MONTHS: u32 = 2;

fn fleet_outbox_retention_cutoff_ms(now_ms: i64) -> Result<i64, RouterLocalStorageError> {
    if now_ms < 0 {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    let now = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .ok_or(RouterLocalStorageError::InvalidRecord)?;
    now.checked_sub_months(chrono::Months::new(FLEET_OUTBOX_RETENTION_MONTHS))
        .map(|cutoff| cutoff.timestamp_millis())
        .ok_or(RouterLocalStorageError::InvalidRecord)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FleetOutboxState {
    Pending {},
    Decided { decision: RequestDecision },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetOutboxEntry {
    pub request: FleetRequest,
    pub state: FleetOutboxState,
}

pub struct FleetOutbox {
    database: RouterLocalDatabase,
}
impl FleetOutbox {
    pub async fn request(
        &self,
        request: &RequestId,
    ) -> Result<Option<FleetOutboxEntry>, RouterLocalStorageError> {
        let id = request.as_str();
        sqlx::query::<sqlx_turso::Turso>("SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE request_id=?")
            .bind(id)
            .fetch_optional(&self.database.pool).await?
            .map(decode_outbox_row)
            .transpose()
    }

    pub async fn settled_directory_page(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<FleetOutboxEntry>, RouterLocalStorageError> {
        let limit = i64::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let rows = sqlx::query::<sqlx_turso::Turso>("SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE request_id>? AND json_extract(state,'$.kind')='decided' AND json_extract(request,'$.operation.kind')='projectDirectoryCreate' ORDER BY request_id LIMIT ?")
            .bind(after)
            .bind(limit)
            .fetch_all(&self.database.pool).await?;
        rows.into_iter().map(decode_outbox_row).collect()
    }
    pub async fn pending_request(
        &self,
        request: &RequestId,
    ) -> Result<Option<FleetOutboxEntry>, RouterLocalStorageError> {
        let id = request.as_str();
        let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
        sqlx::query::<sqlx_turso::Turso>("SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE request_id=? AND state=?")
            .bind(id)
            .bind(state)
            .fetch_optional(&self.database.pool).await?
            .map(decode_outbox_row)
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
    ) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = enqueue_request(&mut transaction, request).await;
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
        let rows = sqlx::query::<sqlx_turso::Turso>("SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE state=? ORDER BY request_id")
            .bind(state)
            .fetch_all(&self.database.pool).await?;
        rows.into_iter().map(decode_outbox_row).collect()
    }
    pub async fn mark_decided(
        &self,
        request: &RequestId,
        decision: &RequestDecision,
        now_ms: i64,
    ) -> Result<(), RouterLocalStorageError> {
        if now_ms < 0 {
            return Err(RouterLocalStorageError::InvalidRecord);
        }
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = request.as_str();
        let row = sqlx::query::<sqlx_turso::Turso>(
            "SELECT canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE request_id=?",
        )
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        let current = decode_entry(
            id,
            &row.try_get::<String, _>("canonical_hash")?,
            &row.try_get::<String, _>("request")?,
            &row.try_get::<String, _>("state")?,
            row.try_get::<Option<i64>, _>("settled_at_ms")?,
        )?;
        match current.state {
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
                let updated = sqlx::query::<sqlx_turso::Turso>(
                    "UPDATE fleet_outbox_requests SET state=?,settled_at_ms=? WHERE request_id=? AND settled_at_ms IS NULL",
                )
                .bind(state)
                .bind(now_ms)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
                if updated.rows_affected() != 1 {
                    transaction.rollback().await?;
                    return Err(RouterLocalStorageError::InvalidRecord);
                }
                transaction.commit().await?;
                Ok(())
            }
        }
    }

    /// Deletes one bounded batch of old settled requests with their creation inputs.
    pub async fn prune_expired(
        &self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, RouterLocalStorageError> {
        if !(1..=MAX_FLEET_OUTBOX_PRUNE_BATCH).contains(&batch_size) {
            return Err(RouterLocalStorageError::InvalidRecord);
        }
        let cutoff_ms = fleet_outbox_retention_cutoff_ms(now_ms)?;
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let invalid_pending = sqlx::query::<sqlx_turso::Turso>(
            "SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE json_extract(state,'$.kind')='pending' AND settled_at_ms IS NOT NULL ORDER BY request_id LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = invalid_pending {
            let error = match decode_outbox_row(row) {
                Err(error) => error,
                Ok(_) => RouterLocalStorageError::InvalidRecord,
            };
            transaction.rollback().await?;
            return Err(error);
        }
        let candidates = sqlx::query::<sqlx_turso::Turso>(
            "SELECT request_id,canonical_hash,request,state,settled_at_ms FROM fleet_outbox_requests WHERE json_extract(state,'$.kind')='decided' AND (settled_at_ms IS NULL OR settled_at_ms<?) ORDER BY COALESCE(settled_at_ms,0),request_id LIMIT ?",
        )
        .bind(cutoff_ms)
        .bind(i64::from(batch_size))
        .fetch_all(&mut *transaction)
        .await?;
        let mut removed = 0_u64;
        for row in candidates {
            let entry = decode_outbox_row(row)?;
            let FleetOutboxState::Decided { .. } = entry.state else {
                transaction.rollback().await?;
                return Err(RouterLocalStorageError::InvalidRecord);
            };
            let request_id = entry.request.request_id.as_str();
            sqlx::query::<sqlx_turso::Turso>(
                "DELETE FROM project_creation_inputs WHERE request_id=?",
            )
            .bind(request_id)
            .execute(&mut *transaction)
            .await?;
            let deleted = sqlx::query::<sqlx_turso::Turso>(
                "DELETE FROM fleet_outbox_requests WHERE request_id=? AND json_extract(state,'$.kind')='decided' AND settled_at_ms<?",
            )
            .bind(request_id)
            .bind(cutoff_ms)
            .execute(&mut *transaction)
            .await?;
            if deleted.rows_affected() != 1 {
                transaction.rollback().await?;
                return Err(RouterLocalStorageError::InvalidRecord);
            }
            removed = removed
                .checked_add(deleted.rows_affected())
                .ok_or(RouterLocalStorageError::InvalidRecord)?;
        }
        transaction.commit().await?;
        Ok(removed)
    }

    pub async fn close(self) {
        self.database.close().await;
    }
}

fn decode_outbox_row(
    row: sqlx_turso::TursoRow,
) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
    let request_id: String = row.try_get("request_id")?;
    let canonical_hash: String = row.try_get("canonical_hash")?;
    let request: String = row.try_get("request")?;
    let state: String = row.try_get("state")?;
    let settled_at_ms: Option<i64> = row.try_get("settled_at_ms")?;
    decode_entry(
        &request_id,
        &canonical_hash,
        &request,
        &state,
        settled_at_ms,
    )
}

fn decode_entry(
    id: &str,
    hash: &str,
    request: &str,
    state: &str,
    settled_at_ms: Option<i64>,
) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
    let request: FleetRequest =
        serde_json::from_str(request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    let canonical = canonical_hash(&request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    if request.request_id.as_str() != id || canonical.as_str() != hash {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    let state: FleetOutboxState =
        serde_json::from_str(state).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    match (&state, settled_at_ms) {
        (FleetOutboxState::Pending {}, None) | (FleetOutboxState::Decided { .. }, Some(_)) => {}
        _ => {
            return Err(RouterLocalStorageError::InvalidOutboxRetentionState {
                request_id: id.to_owned(),
            });
        }
    }
    Ok(FleetOutboxEntry { request, state })
}

async fn enqueue_request(
    transaction: &mut sqlx::Transaction<'_, sqlx_turso::Turso>,
    request: &FleetRequest,
) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
    let id = request.request_id.as_str();
    let hash = canonical_hash(request).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    if let Some(row) = sqlx::query::<sqlx_turso::Turso>("SELECT request,state,canonical_hash,settled_at_ms FROM fleet_outbox_requests WHERE request_id=?")
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await? {
                let stored_hash: String = row.try_get("canonical_hash")?;
                if stored_hash != hash.as_str() { return Err(RouterLocalStorageError::ConflictingRequest); }
                let stored_request: String = row.try_get("request")?;
                let state: String = row.try_get("state")?;
                let settled_at_ms: Option<i64> = row.try_get("settled_at_ms")?;
                return decode_entry(id, &stored_hash, &stored_request, &state, settled_at_ms);
            }
    let hash = hash.as_str();
    let encoded = serde_json::to_string(request)?;
    let state = serde_json::to_string(&FleetOutboxState::Pending {})?;
    sqlx::query::<sqlx_turso::Turso>("INSERT INTO fleet_outbox_requests(request_id,canonical_hash,request,state,settled_at_ms) VALUES (?,?,?,?,NULL)")
        .bind(id)
        .bind(hash)
        .bind(encoded)
        .bind(state)
        .execute(&mut **transaction)
        .await?;
    Ok(FleetOutboxEntry {
        request: request.clone(),
        state: FleetOutboxState::Pending {},
    })
}
