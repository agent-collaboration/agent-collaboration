//! Bounded timer-worker inventory; selection is advisory and transactional claims decide dispatch.
use crate::{AutomationStore, StorageError};
use agent_automation::{DeliveryId, WakeupId};
impl AutomationStore {
    pub async fn due_wakeup_ids(
        &mut self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<WakeupId>, StorageError> {
        validate_batch(now_ms, limit)?;
        let ids:Vec<String>=sqlx::query_scalar("SELECT wakeup_id FROM wakeup_definitions WHERE (wakeup_status='active' AND next_due_at_ms<=?) OR (wakeup_status IN ('active','paused') AND expires_at_ms<=?) ORDER BY COALESCE(next_due_at_ms,expires_at_ms),wakeup_id LIMIT ?")
            .bind(now_ms).bind(now_ms).bind(i64::from(limit)).fetch_all(&mut self.connection).await?;
        ids.into_iter()
            .map(|id| id.try_into().map_err(|_| StorageError::InvalidRecord))
            .collect()
    }
    pub async fn eligible_delivery_ids(
        &mut self,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<DeliveryId>, StorageError> {
        validate_batch(now_ms, limit)?;
        let ids:Vec<String>=sqlx::query_scalar("SELECT delivery_id FROM mailbox_deliveries WHERE delivery_status IN ('pending','retryable') AND eligible_at_ms<=? ORDER BY eligible_at_ms,delivery_id LIMIT ?")
            .bind(now_ms).bind(i64::from(limit)).fetch_all(&mut self.connection).await?;
        ids.into_iter()
            .map(|id| id.try_into().map_err(|_| StorageError::InvalidRecord))
            .collect()
    }
    /// The wake-up worker's next due time strictly after `now_ms`, read with the
    /// same filters as its inventories: an active wake's next firing, an active or
    /// paused wake's expiry, and a pending or retryable delivery's eligibility.
    pub async fn wake_worker_next_due_at_ms(
        &mut self,
        now_ms: i64,
    ) -> Result<Option<i64>, StorageError> {
        Ok(sqlx::query_scalar(
            "SELECT MIN(due) FROM (
                SELECT next_due_at_ms AS due FROM wakeup_definitions WHERE wakeup_status='active' AND next_due_at_ms>?
                UNION ALL SELECT expires_at_ms FROM wakeup_definitions WHERE wakeup_status IN ('active','paused') AND expires_at_ms>?
                UNION ALL SELECT eligible_at_ms FROM mailbox_deliveries WHERE delivery_status IN ('pending','retryable') AND eligible_at_ms>?
            )",
        )
        .bind(now_ms)
        .bind(now_ms)
        .bind(now_ms)
        .fetch_one(&mut self.connection)
        .await?)
    }
}
fn validate_batch(now_ms: i64, limit: u32) -> Result<(), StorageError> {
    if now_ms < 0 || !(1..=100).contains(&limit) {
        Err(StorageError::InvalidRecord)
    } else {
        Ok(())
    }
}
