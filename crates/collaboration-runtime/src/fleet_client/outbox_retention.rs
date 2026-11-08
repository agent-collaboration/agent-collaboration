//! Fleet outbox retention: the worker sleeps until the Fleet database commits or the first
//! moment the retention rule allows a settled request to go, and deletes only then.
use crate::{BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass};
use message_board_storage::{FleetOutbox, RouterLocalStorageError};
use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;

const OUTBOX_PRUNE_BATCH: u32 = 500;

#[derive(Debug, thiserror::Error)]
pub(crate) enum OutboxRetentionError {
    #[error(transparent)]
    Storage(#[from] RouterLocalStorageError),
    /// The due read and the DELETE share one rule; a due outbox that deletes nothing
    /// retries on the loop's backoff instead of re-arming a timer in the past.
    #[error("fleet outbox retention was due but its prune removed nothing")]
    DueButNothingRemoved,
}

/// The loop, subscribed to the Fleet database's commits, and the pass it drives.
pub(crate) fn outbox_retention_loop(
    outbox: &FleetOutbox,
    clock: Arc<dyn Clock>,
) -> (WakeDrivenWorkerLoop, OutboxRetentionPass) {
    let worker_loop = WakeDrivenWorkerLoop::new("fleet_outbox_retention", clock)
        .wake_on(outbox.subscribe_commits());
    let pass = OutboxRetentionPass {
        outbox: outbox.clone(),
        due_ms: None,
    };
    (worker_loop, pass)
}

pub(crate) struct OutboxRetentionPass {
    outbox: FleetOutbox,
    /// The due time the last read found still ahead.
    due_ms: Option<i64>,
}

impl WorkerPass for OutboxRetentionPass {
    type Error = OutboxRetentionError;

    /// Reads the due time and, only when it has passed, deletes one batch. A batch that
    /// deleted rows asks for another, whose read finds the next due time.
    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, Self::Error> {
        let now_ms = now.unix_millis();
        self.due_ms = None;
        observation.record_read("fleet_outbox_retention.due");
        let due_ms = self.outbox.retention_due_ms(now_ms).await?;
        if !due_ms.is_some_and(|due_ms| due_ms <= now_ms) {
            self.due_ms = due_ms;
            return Ok(BatchOutcome::Drained);
        }
        observation.record_read("fleet_outbox_retention.prune");
        if self
            .outbox
            .prune_expired(now_ms, OUTBOX_PRUNE_BATCH)
            .await?
            == 0
        {
            return Err(OutboxRetentionError::DueButNothingRemoved);
        }
        Ok(BatchOutcome::MoreWork)
    }

    async fn next_due(
        &mut self,
        _now: SyncTime,
        _observation: &WorkerObservation,
    ) -> Result<Option<SyncTime>, Self::Error> {
        Ok(self.due_ms.map(SyncTime::from_unix_millis))
    }
}

#[cfg(test)]
#[path = "outbox_retention_test.rs"]
mod tests;
