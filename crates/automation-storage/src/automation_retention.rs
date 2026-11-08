//! Bounded retention for automation history that is not current state.
//!
//! Each class states, once, which rows its rule may ever delete (everything but the age
//! cutoff). Its DELETE and its next-due read share that fragment, so the time the worker
//! wakes is always the time the DELETE first takes a row.
use crate::automation_connection::OneStatementWrite;
use crate::{
    AutomationStore, StorageError,
    automation_event_bounds::{AUTOMATION_EVENT_RETENTION_MONTHS, calendar_months_before},
};

const MAX_AUTOMATION_PRUNE_BATCH: u32 = 500;

/// Every automation history class keeps two calendar months.
const AUTOMATION_RETENTION_MONTHS: u32 = AUTOMATION_EVENT_RETENTION_MONTHS;

/// One kind of automation history a retention rule prunes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AutomationRetentionClass {
    Events,
    SettledMailboxDeliveries,
    OperationReceipts,
    CompletedWorkflowRuns,
    TerminalWakes,
    OldInstructionRevisions,
}

impl AutomationRetentionClass {
    pub const ALL: [Self; 6] = [
        Self::Events,
        Self::SettledMailboxDeliveries,
        Self::OperationReceipts,
        Self::CompletedWorkflowRuns,
        Self::TerminalWakes,
        Self::OldInstructionRevisions,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Events => "automation events",
            Self::SettledMailboxDeliveries => "settled mailbox deliveries",
            Self::OperationReceipts => "operation receipts",
            Self::CompletedWorkflowRuns => "completed workflow runs",
            Self::TerminalWakes => "expired or cancelled wakes",
            Self::OldInstructionRevisions => "old instruction revisions",
        }
    }

    /// The largest batch one prune call may take.
    #[must_use]
    pub const fn max_batch(self) -> u32 {
        match self {
            Self::Events => 1000,
            _ => MAX_AUTOMATION_PRUNE_BATCH,
        }
    }

    const fn statements(self) -> ClassStatements {
        match self {
            Self::Events => EVENTS,
            Self::SettledMailboxDeliveries => MAILBOX_DELIVERIES,
            Self::OperationReceipts => OPERATION_RECEIPTS,
            Self::CompletedWorkflowRuns => WORKFLOW_RUNS,
            Self::TerminalWakes => TERMINAL_WAKES,
            Self::OldInstructionRevisions => INSTRUCTION_REVISIONS,
        }
    }
}

/// A class's DELETE (`?1` cutoff, `?2` batch) and its read of the oldest deletable stamp.
#[derive(Clone, Copy)]
struct ClassStatements {
    prune: &'static str,
    oldest: &'static str,
}

macro_rules! events_eligible {
    () => {
        "FROM automation_events AS event WHERE 1=1"
    };
}
const EVENTS: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM automation_events WHERE event_sequence IN (SELECT event.event_sequence ",
        events_eligible!(),
        " AND event.recorded_at_ms < ?1 ORDER BY event.event_sequence LIMIT ?2)"
    ),
    oldest: concat!("SELECT MIN(event.recorded_at_ms) ", events_eligible!()),
};

// A settled delivery goes once no wake still references it as pending.
macro_rules! mailbox_eligible {
    () => {
        "FROM mailbox_deliveries AS delivery WHERE delivery.delivery_status IN ('accepted','failed','discarded') AND NOT EXISTS (SELECT 1 FROM wakeup_definitions AS wake WHERE wake.pending_delivery_id=delivery.delivery_id)"
    };
}
const MAILBOX_DELIVERIES: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM mailbox_deliveries WHERE rowid IN (SELECT delivery.rowid ",
        mailbox_eligible!(),
        " AND delivery.created_at_ms < ?1 ORDER BY delivery.created_at_ms,delivery.delivery_id LIMIT ?2)"
    ),
    oldest: concat!("SELECT MIN(delivery.created_at_ms) ", mailbox_eligible!()),
};

// Whole replay receipts go after the idempotency window.
macro_rules! receipts_eligible {
    () => {
        "FROM operation_receipts AS receipt WHERE receipt.operation_status IN ('succeeded','failed')"
    };
}
const OPERATION_RECEIPTS: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM operation_receipts WHERE rowid IN (SELECT receipt.rowid ",
        receipts_eligible!(),
        " AND receipt.committed_at_ms < ?1 ORDER BY receipt.committed_at_ms,receipt.operation_id LIMIT ?2)"
    ),
    oldest: concat!("SELECT MIN(receipt.committed_at_ms) ", receipts_eligible!()),
};

// Each schedule keeps its newest finished run.
macro_rules! workflow_runs_eligible {
    () => {
        "FROM workflow_runs AS candidate WHERE candidate.run_status IN ('finished','preparationFailed') AND NOT (candidate.run_status='finished' AND candidate.run_id = (SELECT retained.run_id FROM workflow_runs AS retained WHERE retained.schedule_id=candidate.schedule_id AND retained.run_status='finished' ORDER BY retained.completed_at_ms DESC,retained.run_id DESC LIMIT 1))"
    };
}
const WORKFLOW_RUNS: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM workflow_runs WHERE rowid IN (SELECT candidate.rowid ",
        workflow_runs_eligible!(),
        " AND candidate.completed_at_ms < ?1 ORDER BY candidate.completed_at_ms,candidate.run_id LIMIT ?2)"
    ),
    oldest: concat!(
        "SELECT MIN(candidate.completed_at_ms) ",
        workflow_runs_eligible!()
    ),
};

// An expired or cancelled wake goes once it has no pending delivery, no delivery row and
// no remote request.
macro_rules! terminal_wakes_eligible {
    () => {
        "FROM wakeup_definitions AS wake WHERE wake.wakeup_status IN ('cancelled','expired') AND wake.pending_delivery_id IS NULL AND NOT EXISTS (SELECT 1 FROM mailbox_deliveries AS delivery WHERE delivery.wakeup_id = wake.wakeup_id) AND NOT EXISTS (SELECT 1 FROM remote_wake_requests AS request WHERE request.wakeup_id = wake.wakeup_id)"
    };
}
const TERMINAL_WAKES: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM wakeup_definitions WHERE rowid IN (SELECT wake.rowid ",
        terminal_wakes_eligible!(),
        " AND wake.updated_at_ms < ?1 ORDER BY wake.updated_at_ms,wake.wakeup_id LIMIT ?2)"
    ),
    oldest: concat!(
        "SELECT MIN(wake.updated_at_ms) ",
        terminal_wakes_eligible!()
    ),
};

// Each instruction keeps its current revision.
macro_rules! instruction_revisions_eligible {
    () => {
        "FROM instruction_revisions AS revision JOIN instruction_documents AS document ON document.instruction_id = revision.instruction_id WHERE revision.revision_id != document.current_revision_id"
    };
}
const INSTRUCTION_REVISIONS: ClassStatements = ClassStatements {
    prune: concat!(
        "DELETE FROM instruction_revisions WHERE rowid IN (SELECT revision.rowid ",
        instruction_revisions_eligible!(),
        " AND revision.recorded_at_ms < ?1 ORDER BY revision.recorded_at_ms,revision.revision_id LIMIT ?2)"
    ),
    oldest: concat!(
        "SELECT MIN(revision.recorded_at_ms) ",
        instruction_revisions_eligible!()
    ),
};

// remote_wake_requests stays unpruned until the central-automations cutover.

impl AutomationRetentionClass {
    /// Deletes one bounded batch of this class's rows its rule allows at `now_ms`.
    pub async fn prune(
        self,
        store: &mut AutomationStore,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        if !(1..=self.max_batch()).contains(&batch_size) {
            return Err(StorageError::InvalidRecord);
        }
        let cutoff_ms = calendar_months_before(now_ms, AUTOMATION_RETENTION_MONTHS)?;
        let result = sqlx::query(self.statements().prune)
            .bind(cutoff_ms)
            .bind(i64::from(batch_size))
            .execute_as_transaction(store)
            .await?;
        Ok(result.rows_affected())
    }

    /// The first moment this class's rule allows deleting a row the store holds now, or
    /// `None` when no row could ever go. A protected row is not counted; removing its
    /// protection commits.
    pub async fn due_ms(self, store: &mut AutomationStore) -> Result<Option<i64>, StorageError> {
        let oldest: Option<i64> = sqlx::query_scalar(self.statements().oldest)
            .fetch_one(&mut store.connection)
            .await?;
        oldest
            .map(|stamped_ms| {
                crate::automation_event_bounds::first_deletable_ms(
                    stamped_ms,
                    AUTOMATION_RETENTION_MONTHS,
                )
            })
            .transpose()
    }
}

impl AutomationStore {
    /// Removes a bounded batch strictly before the UTC calendar cutoff; owner records are never pruned.
    pub async fn prune_automation_events(
        &mut self,
        now_ms: i64,
        batch_limit: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::Events
            .prune(self, now_ms, batch_limit)
            .await
    }

    /// Deletes settled wake payload rows that no wake still references as pending.
    pub async fn prune_settled_mailbox_deliveries(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::SettledMailboxDeliveries
            .prune(self, now_ms, batch_size)
            .await
    }

    /// Deletes whole replay receipts after the idempotency retention window.
    pub async fn prune_operation_receipts(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::OperationReceipts
            .prune(self, now_ms, batch_size)
            .await
    }

    /// Deletes one bounded batch of old workflow runs, retaining each schedule's newest finished run.
    pub async fn prune_completed_workflow_runs(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::CompletedWorkflowRuns
            .prune(self, now_ms, batch_size)
            .await
    }

    /// Deletes one bounded batch of expired or cancelled wakes with no retained work.
    pub async fn prune_terminal_wakeups(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::TerminalWakes
            .prune(self, now_ms, batch_size)
            .await
    }

    /// Deletes one bounded batch of old instruction revisions while retaining each current revision.
    pub async fn prune_old_instruction_revisions(
        &mut self,
        now_ms: i64,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        AutomationRetentionClass::OldInstructionRevisions
            .prune(self, now_ms, batch_size)
            .await
    }
}
