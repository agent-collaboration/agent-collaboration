//! Bounded retention for automation history that is not current state.
use crate::{AutomationStore, StorageError, automation_event_bounds::calendar_months_before};

const MAX_AUTOMATION_PRUNE_BATCH: u32 = 500;

/// Retains settled mailbox deliveries for two calendar months.
pub(crate) const MAILBOX_DELIVERY_RETENTION_MONTHS: u32 = 2;

/// Retains terminal operation receipts for two calendar months.
pub(crate) const OPERATION_RECEIPT_RETENTION_MONTHS: u32 = 2;

/// Retains completed workflow runs for two calendar months.
pub(crate) const WORKFLOW_RUN_RETENTION_MONTHS: u32 = 2;

/// Retains expired and cancelled wake definitions for two calendar months.
pub(crate) const WAKEUP_DEFINITION_RETENTION_MONTHS: u32 = 2;

/// Retains non-current instruction revisions for two calendar months.
pub(crate) const INSTRUCTION_REVISION_RETENTION_MONTHS: u32 = 2;

// remote_wake_requests stays unpruned until the central-automations cutover.

impl AutomationStore {
    /// Deletes settled wake payload rows that no wake still references as pending.
    pub async fn prune_settled_mailbox_deliveries(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        validate_batch_size(batch_size)?;
        let cutoff_ms = calendar_months_before(now_ms, MAILBOX_DELIVERY_RETENTION_MONTHS)?;
        let result = sqlx::query!(
            "DELETE FROM mailbox_deliveries WHERE rowid IN (SELECT delivery.rowid FROM mailbox_deliveries AS delivery WHERE delivery.created_at_ms < ? AND delivery.delivery_status IN ('accepted','failed','discarded') AND NOT EXISTS (SELECT 1 FROM wakeup_definitions AS wake WHERE wake.pending_delivery_id=delivery.delivery_id) ORDER BY delivery.created_at_ms,delivery.delivery_id LIMIT ?)",
            cutoff_ms,
            i64::from(batch_size)
        )
        .execute(&mut self.connection)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes whole replay receipts after the idempotency retention window.
    pub async fn prune_operation_receipts(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        validate_batch_size(batch_size)?;
        let cutoff_ms = calendar_months_before(now_ms, OPERATION_RECEIPT_RETENTION_MONTHS)?;
        let result = sqlx::query(
            "DELETE FROM operation_receipts
              WHERE rowid IN (
                  SELECT rowid
                    FROM operation_receipts
                   WHERE committed_at_ms < ?1
                     AND operation_status IN ('succeeded','failed')
                   ORDER BY committed_at_ms,operation_id
                   LIMIT ?2
              )",
        )
        .bind(cutoff_ms)
        .bind(i64::from(batch_size))
        .execute(&mut self.connection)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes one bounded batch of completed workflow runs older than the cutoff.
    pub async fn prune_completed_workflow_runs(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        validate_batch_size(batch_size)?;
        let cutoff_ms = calendar_months_before(now_ms, WORKFLOW_RUN_RETENTION_MONTHS)?;
        let result = sqlx::query(
            "DELETE FROM workflow_runs
              WHERE rowid IN (
                  SELECT rowid
                    FROM workflow_runs
                   WHERE completed_at_ms < ?1
                     AND run_status IN ('finished','preparationFailed')
                   ORDER BY completed_at_ms,run_id
                   LIMIT ?2
              )",
        )
        .bind(cutoff_ms)
        .bind(i64::from(batch_size))
        .execute(&mut self.connection)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes one bounded batch of expired or cancelled wakes with no retained work.
    pub async fn prune_terminal_wakeups(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        validate_batch_size(batch_size)?;
        let cutoff_ms = calendar_months_before(now_ms, WAKEUP_DEFINITION_RETENTION_MONTHS)?;
        let result = sqlx::query(
            "DELETE FROM wakeup_definitions
              WHERE rowid IN (
                  SELECT wake.rowid
                    FROM wakeup_definitions AS wake
                   WHERE wake.updated_at_ms < ?1
                     AND wake.wakeup_status IN ('cancelled','expired')
                     AND wake.pending_delivery_id IS NULL
                     AND NOT EXISTS (
                         SELECT 1 FROM mailbox_deliveries AS delivery
                          WHERE delivery.wakeup_id = wake.wakeup_id
                     )
                     AND NOT EXISTS (
                         SELECT 1 FROM remote_wake_requests AS request
                          WHERE request.wakeup_id = wake.wakeup_id
                     )
                   ORDER BY wake.updated_at_ms,wake.wakeup_id
                   LIMIT ?2
              )",
        )
        .bind(cutoff_ms)
        .bind(i64::from(batch_size))
        .execute(&mut self.connection)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes one bounded batch of old instruction revisions while retaining each current revision.
    pub async fn prune_old_instruction_revisions(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        validate_batch_size(batch_size)?;
        let cutoff_ms = calendar_months_before(now_ms, INSTRUCTION_REVISION_RETENTION_MONTHS)?;
        let result = sqlx::query(
            "DELETE FROM instruction_revisions
              WHERE rowid IN (
                  SELECT revision.rowid
                    FROM instruction_revisions AS revision
                    JOIN instruction_documents AS document
                      ON document.instruction_id = revision.instruction_id
                   WHERE revision.recorded_at_ms < ?1
                     AND revision.revision_id != document.current_revision_id
                   ORDER BY revision.recorded_at_ms,revision.revision_id
                   LIMIT ?2
              )",
        )
        .bind(cutoff_ms)
        .bind(i64::from(batch_size))
        .execute(&mut self.connection)
        .await?;
        Ok(result.rows_affected())
    }
}

fn validate_batch_size(batch_size: u32) -> Result<(), StorageError> {
    if (1..=MAX_AUTOMATION_PRUNE_BATCH).contains(&batch_size) {
        Ok(())
    } else {
        Err(StorageError::InvalidRecord)
    }
}
