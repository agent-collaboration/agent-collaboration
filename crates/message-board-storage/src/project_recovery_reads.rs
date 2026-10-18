//! Creation recovery and terminal voiding select only the ledgers and facts they use.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

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
            .fetch_optional(&mut *self.transaction).await?.flatten()
            .map(|request| RequestId::try_from(request).map_err(|_| ProjectStorageError::InvalidFeed))
            .transpose()
    }
    pub async fn creation_fact_key(
        &mut self,
        request: &RequestId,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        let request = request.as_str();
        sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities INDEXED BY creation_fact_by_request WHERE kind = 'fact' AND json_extract(payload, '$.record.kind.kind') = 'lifecycle' AND json_extract(payload, '$.record.kind.fact.kind') = 'projectCreated' AND json_extract(payload, '$.record.kind.fact.creatingRequest') = ? ORDER BY id LIMIT 1"#, request).fetch_optional(&mut *self.transaction).await?
            .map(|id| reference(RecordKind::Fact, id)).transpose()
    }
    pub async fn creation_fact_page_keys(
        &mut self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'fact' AND json_extract(payload, '$.record.kind.kind') = 'lifecycle' AND json_extract(payload, '$.record.kind.fact.kind') = 'projectCreated' AND id > ? ORDER BY id LIMIT ?"#, after, limit).fetch_all(&mut *self.transaction).await?
            .into_iter().map(|id| reference(RecordKind::Fact, id)).collect()
    }
    pub async fn project_rejection_request(
        &mut self,
    ) -> Result<Option<RequestLedgerEntry>, ProjectStorageError> {
        let id = sqlx_turso::query_scalar!(r#"SELECT request_id AS "request_id!: String" FROM request_ledger WHERE json_extract(decision, '$.kind') = 'voided' AND json_extract(decision, '$.cause.kind') = 'projectRejected' ORDER BY request_id LIMIT 1"#).fetch_optional(&mut *self.transaction).await?;
        let Some(id) = id else {
            return Ok(None);
        };
        let key = reference(RecordKind::Request, id)?;
        match self.record(&key).await? {
            Some(Record::Request(ledger))
                if matches!(
                    ledger.decision,
                    RequestDecision::Voided {
                        cause: VoidCause::ProjectRejected { .. }
                    }
                ) =>
            {
                Ok(Some(ledger))
            }
            _ => Err(ProjectStorageError::InvalidRecord {
                record: key,
                field: "decision",
            }),
        }
    }
}
impl ProjectWriteTransaction {
    pub async fn accepted_request_page_keys(
        &mut self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        sqlx_turso::query_scalar!(r#"SELECT request_id AS "request_id!: String" FROM request_ledger WHERE json_extract(decision, '$.kind') = 'accepted' AND request_id > ? ORDER BY request_id LIMIT ?"#, after, limit).fetch_all(&mut *self.transaction).await?
            .into_iter().map(|id| reference(RecordKind::Request, id)).collect()
    }
}
