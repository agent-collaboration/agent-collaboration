//! Native held-mail discovery for the reader readiness coordinator.
use crate::{InteractionStore, StorageError, push_record_rows};
use collaboration_protocol::{PushId, PushRecord, SessionRef};

impl InteractionStore {
    /// Readiness needs unscheduled Held identities; the SDK owns scheduled backoff retries.
    pub async fn held_mail_push_ids(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushId>, StorageError> {
        let mut connection = self.writer.lock().await;
        let service_id = String::from(target.endpoint.service_id.clone());
        let endpoint_id = String::from(target.endpoint.endpoint_id.clone());
        let session_id = String::from(target.session_id.clone());
        let rows =
            sqlx_turso::query_file!("src/held_mail_ids.sql", service_id, endpoint_id, session_id)
                .fetch_all(&mut *connection)
                .await?;
        rows.into_iter()
            .map(|row| PushId::try_from(row.push_id).map_err(|_| StorageError::InvalidStoredRecord))
            .collect()
    }

    pub async fn held_mail_targets(&self) -> Result<Vec<SessionRef>, StorageError> {
        let mut connection = self.writer.lock().await;
        let rows = sqlx_turso::query!(
            "SELECT DISTINCT target_service_id AS \"service!: String\",target_endpoint_id AS \"endpoint!: String\",target_session_id AS \"session!: String\" FROM router_pushes WHERE retention='mailTerminalAge' AND delivery_state='held' ORDER BY target_service_id,target_endpoint_id,target_session_id"
        ).fetch_all(&mut *connection).await?;
        rows.into_iter()
            .map(|row| push_record_rows::session_from_parts(row.service, row.endpoint, row.session))
            .collect()
    }

    pub async fn list_held_mail_push_records(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let service_id = String::from(target.endpoint.service_id.clone());
        let endpoint_id = String::from(target.endpoint.endpoint_id.clone());
        let session_id = String::from(target.session_id.clone());
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND retention='mailTerminalAge' AND delivery_state='held' ORDER BY created_at,push_id",
            service_id,
            endpoint_id,
            session_id
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }
}
