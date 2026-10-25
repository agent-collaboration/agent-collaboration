//! Recipient retry counts are item-local and share the library's outcome transaction.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};
use collaboration_protocol::PushId;
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use sqlx_turso::TursoConnection;

async fn read_attempts(
    connection: &mut TursoConnection,
    item: &PushId,
) -> Result<u32, StorageError> {
    let row = sqlx_turso::query!(
        "SELECT retry_attempts AS \"retry_attempts!: i64\" FROM local_delivery_retries WHERE item_id=?",
        item.as_str()
    )
    .fetch_optional(connection)
    .await?;
    row.map(|row| u32::try_from(row.retry_attempts).map_err(|_| StorageError::InvalidStoredRecord))
        .transpose()
        .map(|attempts| attempts.unwrap_or(0))
}

impl InteractionStore {
    pub async fn local_delivery_retry_attempts(&self, item: &PushId) -> Result<u32, StorageError> {
        read_attempts(&mut *self.writer.lock().await, item).await
    }
}

pub async fn record_local_delivery_retry_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &PushId,
    increment_attempts: bool,
    scheduled_retry_at: Option<DateTime<Utc>>,
) -> Result<(), StorageError> {
    let previous = read_attempts(scope.executor(), item).await?;
    let attempts = i64::from(previous.saturating_add(u32::from(increment_attempts)));
    let scheduled = scheduled_retry_at.map(crate::push_record_rows::serialize_timestamp);
    // Replacing the nullable deadline on every outcome also clears a preceding schedule.
    sqlx_turso::query!(
        "INSERT INTO local_delivery_retries(item_id,retry_attempts,scheduled_retry_at) VALUES (?,?,?) ON CONFLICT(item_id) DO UPDATE SET retry_attempts=excluded.retry_attempts,scheduled_retry_at=excluded.scheduled_retry_at",
        item.as_str(), attempts, scheduled
    ).execute(scope.executor()).await?;
    Ok(())
}

pub async fn clear_local_delivery_retry_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &PushId,
) -> Result<(), StorageError> {
    sqlx_turso::query!(
        "DELETE FROM local_delivery_retries WHERE item_id=?",
        item.as_str()
    )
    .execute(scope.executor())
    .await?;
    Ok(())
}
