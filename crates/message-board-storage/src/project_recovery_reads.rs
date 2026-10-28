//! Creation recovery selects immutable request keys and facts.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{RecordId, RecordKind, RecordRef, RequestId};

fn reference(kind: RecordKind, id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
impl ProjectSnapshot {
    pub async fn project_creation_request(
        &mut self,
    ) -> Result<Option<RequestId>, ProjectStorageError> {
        let project = self.project_id.as_str();
        // The current ProjectState envelope may belong to a later update.
        // The first project version retains the original outbox request key.
        sqlx_turso::query_scalar!(r#"SELECT request_id AS "request_id: String" FROM project_feed WHERE record_kind = 'project' AND record_id = ? ORDER BY position LIMIT 1"#, project)
            .fetch_optional(&mut self.transaction).await?.flatten()
            .map(|request| RequestId::try_from(request).map_err(|_| ProjectStorageError::InvalidFeed))
            .transpose()
    }
    pub async fn creation_fact_key(
        &mut self,
        request: &RequestId,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        let request = request.as_str();
        sqlx_turso::query_scalar!(r#"SELECT fact_id AS "id!: String" FROM facts WHERE json_extract(fact, '$.kind') = 'lifecycle' AND json_extract(fact, '$.fact.kind') = 'projectCreated' AND json_extract(fact, '$.fact.creatingRequest') = ? ORDER BY fact_id LIMIT 1"#, request).fetch_optional(&mut self.transaction).await?
            .map(|id| reference(RecordKind::Fact, id)).transpose()
    }
    pub async fn creation_fact_page_keys(
        &mut self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        sqlx_turso::query_scalar!(r#"SELECT fact_id AS "id!: String" FROM facts WHERE json_extract(fact, '$.kind') = 'lifecycle' AND json_extract(fact, '$.fact.kind') = 'projectCreated' AND fact_id > ? ORDER BY fact_id LIMIT ?"#, after, limit).fetch_all(&mut self.transaction).await?
            .into_iter().map(|id| reference(RecordKind::Fact, id)).collect()
    }
}
