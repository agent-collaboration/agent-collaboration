//! Keep complete creation input until the project's own creating ledger can replay it.
use super::{FleetOutbox, FleetOutboxEntry, decode_entry, enqueue_request};
use crate::RouterLocalStorageError;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::{
    ProjectCreateRequest, RecordedActor, RequestId, request_canonical::creation_hash,
};
use sqlx::Row;

impl FleetOutbox {
    pub async fn enqueue_creation(
        &self,
        directory: &FleetRequest,
        input: &ProjectCreateRequest,
    ) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
        validate_directory(input, directory)?;
        let hash = creation_hash(input).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let queued = enqueue_request(&mut transaction, directory).await?;
            let id = input.request_id.as_str();
            if let Some(row) = sqlx_turso::query!(r#"SELECT canonical_hash AS "canonical_hash!: String", input AS "input!: String" FROM project_creation_inputs WHERE request_id = ?"#, id)
                .fetch_optional(&mut *transaction).await? {
                decode_input(&row.input, &row.canonical_hash, &queued)?;
                if row.canonical_hash != hash.as_str() {
                    return Err(RouterLocalStorageError::ConflictingRequest);
                }
            } else {
                let hash = hash.as_str();
                let encoded = serde_json::to_string(input)?;
                sqlx_turso::query!("INSERT INTO project_creation_inputs(request_id, canonical_hash, input) VALUES (?, ?, ?)", id, hash, encoded)
                    .execute(&mut *transaction).await?;
            }
            Ok(queued)
        }.await;
        match result {
            Ok(queued) => {
                transaction.commit().await?;
                Ok(queued)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    pub async fn creation_input(
        &self,
        request: &RequestId,
    ) -> Result<Option<ProjectCreateRequest>, RouterLocalStorageError> {
        let id = request.as_str();
        let row = sqlx_turso::query!(r#"SELECT canonical_hash AS "canonical_hash!: String", input AS "input!: String" FROM project_creation_inputs WHERE request_id = ?"#, id)
            .fetch_optional(&self.database.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let queued = self
            .request(request)
            .await?
            .ok_or(RouterLocalStorageError::InvalidRecord)?;
        Ok(Some(decode_input(
            &row.input,
            &row.canonical_hash,
            &queued,
        )?))
    }

    pub async fn accepted_creation_page(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<ProjectCreateRequest>, RouterLocalStorageError> {
        let limit = i64::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let rows = sqlx::query::<sqlx_turso::Turso>("SELECT c.canonical_hash AS creation_hash,c.input,r.request_id,r.canonical_hash,r.request,r.state,r.settled_at_ms FROM project_creation_inputs c JOIN fleet_outbox_requests r ON r.request_id=c.request_id WHERE c.request_id>? AND json_extract(r.state,'$.kind')='decided' AND json_extract(r.state,'$.decision.kind')='accepted' ORDER BY c.request_id LIMIT ?")
            .bind(after)
            .bind(limit)
            .fetch_all(&self.database.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let creation_hash: String = row.try_get("creation_hash")?;
                let input: String = row.try_get("input")?;
                let request_id: String = row.try_get("request_id")?;
                let canonical_hash: String = row.try_get("canonical_hash")?;
                let request: String = row.try_get("request")?;
                let state: String = row.try_get("state")?;
                let settled_at_ms: Option<i64> = row.try_get("settled_at_ms")?;
                let queued = decode_entry(
                    &request_id,
                    &canonical_hash,
                    &request,
                    &state,
                    settled_at_ms,
                )?;
                decode_input(&input, &creation_hash, &queued)
            })
            .collect()
    }
}

fn decode_input(
    encoded: &str,
    hash: &str,
    queued: &FleetOutboxEntry,
) -> Result<ProjectCreateRequest, RouterLocalStorageError> {
    let input: ProjectCreateRequest =
        serde_json::from_str(encoded).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    let canonical = creation_hash(&input).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
    if canonical.as_str() != hash {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    validate_directory(&input, &queued.request)?;
    Ok(input)
}

fn validate_directory(
    input: &ProjectCreateRequest,
    directory: &FleetRequest,
) -> Result<(), RouterLocalStorageError> {
    if directory.request_id != input.request_id
        || directory.actor != RecordedActor::declared(input.actor.clone())
        || directory.acting_for != input.acting_for
        || directory.protocol_version != input.protocol_version
        || !matches!(&directory.operation, FleetOperation::ProjectDirectoryCreate { project, board, title }
            if project == &input.project_id && board == &input.board_id && title == &input.title)
    {
        return Err(RouterLocalStorageError::InvalidRecord);
    }
    Ok(())
}
