//! Session context selects only this session's acknowledged-scope keys.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{RecordId, RecordKind, RecordRef, SessionRef};

impl ProjectSnapshot {
    pub async fn scope_acknowledgement_page_keys(
        &mut self,
        session: &SessionRef,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let service = session.endpoint.service_id.as_str();
        let endpoint = session.endpoint.endpoint_id.as_str();
        let session = session.session_id.as_str();
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query_file_scalar!(
            "src/project_context_scope.sql",
            service,
            endpoint,
            session,
            after,
            limit
        )
        .fetch_all(&mut self.transaction)
        .await?;
        rows.into_iter()
            .map(|id| {
                Ok(RecordRef {
                    kind: RecordKind::Fact,
                    id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
}
