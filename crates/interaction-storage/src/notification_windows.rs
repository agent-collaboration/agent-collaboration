//! Rolling recipient deadlines share the physical interaction-store writer.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};
use collaboration_protocol::{SessionRef, limits::NOTIFICATION_INTERVAL};
use sqlx_turso::TursoConnection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationWindow {
    pub last_delivered_at: DateTime<Utc>,
    pub flush_due_at: Option<DateTime<Utc>>,
}
impl NotificationWindow {
    pub fn next_delivery_at(&self) -> Result<DateTime<Utc>, StorageError> {
        self.last_delivered_at
            .checked_add_signed(chrono::Duration::seconds(i64::from(NOTIFICATION_INTERVAL)))
            .ok_or(StorageError::InvalidStoredRecord)
    }
}
struct NotificationWindowRow {
    last_delivered_at_ms: i64,
    flush_due_at_ms: Option<i64>,
}
impl NotificationWindowRow {
    fn decode(self) -> Result<NotificationWindow, StorageError> {
        let window = NotificationWindow {
            last_delivered_at: DateTime::from_timestamp_millis(self.last_delivered_at_ms)
                .ok_or(StorageError::InvalidStoredRecord)?,
            flush_due_at: self
                .flush_due_at_ms
                .map(|ms| {
                    DateTime::from_timestamp_millis(ms).ok_or(StorageError::InvalidStoredRecord)
                })
                .transpose()?,
        };
        let next_delivery_at = window.next_delivery_at()?;
        if window
            .flush_due_at
            .is_some_and(|due| due != next_delivery_at)
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        Ok(window)
    }
}
pub(crate) fn recipient_key(target: &SessionRef) -> Result<String, StorageError> {
    serde_json::to_string(target).map_err(|_| StorageError::InvalidRecord)
}
pub(crate) async fn read_notification_window(
    connection: &mut TursoConnection,
    target: &SessionRef,
) -> Result<Option<NotificationWindow>, StorageError> {
    let key = recipient_key(target)?;
    sqlx_turso::query_as!(NotificationWindowRow,
        "SELECT last_delivered_at_ms AS \"last_delivered_at_ms!: i64\",flush_due_at_ms AS \"flush_due_at_ms?: i64\" FROM notification_windows WHERE recipient_key=?",key)
        .fetch_optional(connection).await?.map(NotificationWindowRow::decode).transpose()
}
pub(crate) async fn write_notification_delivery(
    connection: &mut TursoConnection,
    target: &SessionRef,
    now: DateTime<Utc>,
) -> Result<(), StorageError> {
    let key = recipient_key(target)?;
    let now_ms = now.timestamp_millis();
    sqlx_turso::query!("INSERT INTO notification_windows(recipient_key,last_delivered_at_ms,flush_due_at_ms) VALUES(?,?,NULL) ON CONFLICT(recipient_key) DO UPDATE SET last_delivered_at_ms=MAX(last_delivered_at_ms,excluded.last_delivered_at_ms),flush_due_at_ms=NULL",key,now_ms)
        .execute(connection).await?;
    Ok(())
}
impl InteractionStore {
    pub async fn notification_window(
        &self,
        target: &SessionRef,
    ) -> Result<Option<NotificationWindow>, StorageError> {
        read_notification_window(&mut *self.writer.lock().await, target).await
    }
    pub async fn record_notification_delivery(
        &self,
        target: &SessionRef,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        write_notification_delivery(&mut *self.writer.lock().await, target, now).await
    }
    pub async fn hold_notification_window(
        &self,
        target: &SessionRef,
    ) -> Result<DateTime<Utc>, StorageError> {
        let mut connection = self.writer.lock().await;
        let window = read_notification_window(&mut connection, target)
            .await?
            .ok_or(StorageError::InvalidRecord)?;
        let due = window.next_delivery_at()?;
        let due_ms = due.timestamp_millis();
        let key = recipient_key(target)?;
        sqlx_turso::query!(
            "UPDATE notification_windows SET flush_due_at_ms=? WHERE recipient_key=?",
            due_ms,
            key
        )
        .execute(&mut *connection)
        .await?;
        Ok(due)
    }
    pub async fn clear_notification_flush(&self, target: &SessionRef) -> Result<(), StorageError> {
        let key = recipient_key(target)?;
        sqlx_turso::query!(
            "UPDATE notification_windows SET flush_due_at_ms=NULL WHERE recipient_key=?",
            key
        )
        .execute(&mut *self.writer.lock().await)
        .await?;
        Ok(())
    }
}
