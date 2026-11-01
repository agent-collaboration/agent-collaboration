//! Capture previous-process attempt identities before any new mail is admitted.
use crate::{InteractionStore, StorageError};
use collaboration_protocol::PushId;

struct AttemptedPushIdRow {
    push_id: String,
}
impl InteractionStore {
    pub async fn notification_startup_attempt_ids(&self) -> Result<Vec<PushId>, StorageError> {
        let mut connection = self.writer.lock().await;
        // No kind decode here: a damaged kind column must not prevent healthy
        // subscription owners from starting and retrying their own recovery.
        let rows = sqlx_turso::query_as!(AttemptedPushIdRow, "SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE delivery_state='attempted' ORDER BY push_id").fetch_all(&mut *connection).await?;
        let mut attempts = Vec::with_capacity(rows.len());
        for row in rows {
            match PushId::try_from(row.push_id) {
                Ok(id) => attempts.push(id),
                Err(error) => tracing::warn!(%error, "invalid startup attempt identity rejected"),
            }
        }
        Ok(attempts)
    }
}
