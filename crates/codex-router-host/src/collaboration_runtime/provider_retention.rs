//! Host-owned provider operation retention and its shutdown-bound worker. The worker
//! sleeps until the operation store commits, a live operation publishes its result, or the
//! first moment the retention rule allows a settled operation to go, and deletes only then.
use super::{CollaborationRuntime, io};
use crate::ExternalProviderSupervisor;
use collaboration_runtime::{
    BatchOutcome, ProviderOperationStore, ProviderOperationStoreError, WakeDrivenWorkerLoop,
    WorkerObservation, WorkerPass, provider_operation_retention_cutoff_ms,
};
use runtime_primitives::{Clock, SyncTime};
use std::{num::NonZeroU32, sync::Arc};
use tokio::sync::Mutex;

impl CollaborationRuntime {
    pub async fn configure_provider_operation_retention(
        &mut self,
        retention_days: Option<NonZeroU32>,
        clock: Arc<dyn Clock>,
    ) -> io::Result<u64> {
        let now_ms = clock.now().unix_millis();
        let cutoff_ms = provider_operation_retention_cutoff_ms(now_ms, retention_days)
            .map_err(io::Error::other)?;
        let protected = self
            .external_provider_supervisor
            .as_ref()
            .map(|supervisor| supervisor.live_operation_ids())
            .unwrap_or_default();
        let Some(store) = &self.provider_store else {
            return Ok(0);
        };
        let pruned = {
            let mut store = store.lock().await;
            let pruned = store
                .prune_terminal_before(cutoff_ms, &protected)
                .await
                .map_err(io::Error::other)?;
            store
                .incremental_vacuum_step()
                .await
                .map_err(io::Error::other)?;
            pruned
        };
        let (worker_loop, mut pass) = provider_retention_loop(
            Arc::clone(store),
            self.external_provider_supervisor.clone(),
            retention_days,
            clock,
        )
        .await;
        let shutdown = self.shutdown.clone();
        self.provider_retention = Some(tokio::spawn(async move {
            worker_loop.run(&mut pass, shutdown).await;
        }));
        Ok(pruned)
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ProviderRetentionError {
    #[error(transparent)]
    Storage(#[from] ProviderOperationStoreError),
    /// The due read and the prune share one rule; a due store whose prune removed nothing
    /// retries on the loop's backoff instead of re-arming a timer in the past.
    #[error("provider operation retention was due but its prune removed nothing")]
    DueButNothingRemoved,
}

/// The loop, subscribed to the store's commits and the supervisor's completions before
/// its first read, and the pass it drives.
pub(super) async fn provider_retention_loop(
    store: Arc<Mutex<ProviderOperationStore>>,
    supervisor: Option<Arc<ExternalProviderSupervisor>>,
    retention_days: Option<NonZeroU32>,
    clock: Arc<dyn Clock>,
) -> (WakeDrivenWorkerLoop, ProviderRetentionPass) {
    let commits = store.lock().await.subscribe_commits();
    let mut worker_loop =
        WakeDrivenWorkerLoop::new("provider_operation_retention", clock).wake_on(commits);
    if let Some(supervisor) = &supervisor {
        worker_loop = worker_loop.wake_on(supervisor.subscribe_completions());
    }
    let pass = ProviderRetentionPass {
        store,
        supervisor,
        retention_days,
        due_ms: None,
        #[cfg(test)]
        prunes_entered_at: Arc::default(),
    };
    (worker_loop, pass)
}

pub(super) struct ProviderRetentionPass {
    store: Arc<Mutex<ProviderOperationStore>>,
    /// Holds the operations still live in memory, which retention must keep.
    supervisor: Option<Arc<ExternalProviderSupervisor>>,
    retention_days: Option<NonZeroU32>,
    /// The due time the last read found still ahead.
    due_ms: Option<i64>,
    /// The pass's own time at each prune it entered, as the loop read it on waking.
    #[cfg(test)]
    prunes_entered_at: Arc<std::sync::Mutex<Vec<SyncTime>>>,
}

impl WorkerPass for ProviderRetentionPass {
    type Error = ProviderRetentionError;

    /// Reads the live operations, then the due time, and only when it has passed deletes
    /// one batch. The loop marked the completion signal seen before this pass, so an
    /// operation that publishes its result after the read wakes another pass.
    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, Self::Error> {
        let now_ms = now.unix_millis();
        self.due_ms = None;
        let protected = self
            .supervisor
            .as_ref()
            .map(|supervisor| supervisor.live_operation_ids())
            .unwrap_or_default();
        let mut store = self.store.lock().await;
        observation.record_read("provider_operation_retention.due");
        let due_ms = store
            .retention_due_ms(now_ms, self.retention_days, &protected)
            .await?;
        if !due_ms.is_some_and(|due_ms| due_ms <= now_ms) {
            self.due_ms = due_ms;
            return Ok(BatchOutcome::Drained);
        }
        observation.record_read("provider_operation_retention.prune");
        #[cfg(test)]
        if let Ok(mut entered) = self.prunes_entered_at.lock() {
            entered.push(now);
        }
        let cutoff_ms = provider_operation_retention_cutoff_ms(now_ms, self.retention_days)?;
        if store.prune_terminal_before(cutoff_ms, &protected).await? == 0 {
            return Err(ProviderRetentionError::DueButNothingRemoved);
        }
        store.incremental_vacuum_step().await?;
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
#[path = "provider_retention_test.rs"]
mod tests;
