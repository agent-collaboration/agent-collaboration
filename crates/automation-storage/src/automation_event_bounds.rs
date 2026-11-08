//! Retention boundaries use two calendar months, independent of whether cleanup has already run.
use crate::{AutomationRetentionClass, AutomationStore, StorageError};

/// Retains automation events for two calendar months.
pub(crate) const AUTOMATION_EVENT_RETENTION_MONTHS: u32 =
    crate::automation_retention::AUTOMATION_RETENTION_MONTHS;

impl AutomationStore {
    /// Removes a bounded batch strictly before the UTC calendar cutoff; owner records are never pruned.
    pub async fn prune_automation_events(
        &mut self,
        now_ms: i64,
        batch_limit: u32,
    ) -> Result<u64, StorageError> {
        self.prune_retention_class(AutomationRetentionClass::Events, now_ms, batch_limit)
            .await
    }
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
