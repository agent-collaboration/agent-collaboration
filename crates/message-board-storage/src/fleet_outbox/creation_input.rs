//! Keep complete creation input until the project's own creating ledger can replay it.
use super::{FleetOutbox, FleetOutboxEntry, decode_entry, enqueue_request};
use crate::RouterLocalStorageError;
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::{
    ProjectCreateRequest, RecordedActor, RequestId, request_canonical::creation_hash,
};

impl FleetOutbox {
    pub async fn enqueue_creation(
        &self,
        directory: &FleetRequest,
        ids: &FleetIds,
        input: &ProjectCreateRequest,
    ) -> Result<FleetOutboxEntry, RouterLocalStorageError> {
        validate_directory(input, directory)?;
        let hash = creation_hash(input).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let _guard = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let queued = enqueue_request(&mut transaction, directory, ids).await?;
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
        let rows = sqlx_turso::query!(r#"SELECT c.canonical_hash AS "creation_hash!: String", c.input AS "input!: String", r.request_id AS "request_id!: String", r.canonical_hash AS "canonical_hash!: String", r.request AS "request!: String", r.ids AS "ids!: String", r.state AS "state!: String" FROM project_creation_inputs c JOIN fleet_outbox_requests r ON r.request_id = c.request_id WHERE c.request_id > ? AND json_extract(r.state, '$.kind') = 'decided' AND json_extract(r.state, '$.decision.kind') = 'accepted' ORDER BY c.request_id LIMIT ?"#, after, limit)
            .fetch_all(&self.database.pool).await?;
        rows.into_iter()
            .map(|row| {
                let queued = decode_entry(
                    &row.request_id,
                    &row.canonical_hash,
                    &row.request,
                    &row.ids,
                    &row.state,
                )?;
                decode_input(&row.input, &row.creation_hash, &queued)
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
