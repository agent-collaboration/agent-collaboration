//! Retention boundaries use two calendar months, independent of whether cleanup has already run.
use crate::{AutomationStore, StorageError};

/// Retains automation events, and every other automation history class, for two calendar months.
pub(crate) const AUTOMATION_EVENT_RETENTION_MONTHS: u32 = 2;

impl AutomationStore {
    pub async fn automation_event_floor(
        &mut self,
        now_ms: i64,
    ) -> Result<Option<(i64, i64)>, StorageError> {
        let cutoff = calendar_months_before(now_ms, AUTOMATION_EVENT_RETENTION_MONTHS)?;
        let first=sqlx::query_as("SELECT event_sequence,recorded_at_ms FROM automation_events WHERE recorded_at_ms>=? ORDER BY event_sequence LIMIT 1").bind(cutoff).fetch_optional(&mut self.connection).await?;
        Ok(first)
    }
}
pub(crate) fn calendar_months_before(now_ms: i64, months: u32) -> Result<i64, StorageError> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .and_then(|now| sqlite_storage_maintenance::months_before(now, months))
        .map(|cutoff| cutoff.timestamp_millis())
        .ok_or(StorageError::InvalidRecord)
}

/// The first millisecond at which a row stamped `stamped_ms` falls before the cutoff.
pub(crate) fn first_deletable_ms(stamped_ms: i64, months: u32) -> Result<i64, StorageError> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(stamped_ms)
        .and_then(|stamped| sqlite_storage_maintenance::first_deletable_at(stamped, months))
        .map(|due| due.timestamp_millis())
        .ok_or(StorageError::InvalidRecord)
}

pub(crate) fn retention_cutoff(now_ms: i64) -> Result<i64, StorageError> {
    calendar_months_before(now_ms, AUTOMATION_EVENT_RETENTION_MONTHS)
}
