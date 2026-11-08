//! The single local worker applies ordered automation operations without dispatching native input.
use automation_storage::{AutomationStore, OwedPushOperation, PushOperation, PushOperationId};
use collaboration_protocol::PushId;
use interaction_storage::InteractionStore;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum PushHandoffError {
    #[error("cannot read automation's owed push operations")]
    Read(#[source] Box<automation_storage::StorageError>),
    #[error("cannot apply push operation {operation_id:?} for {push_id:?}")]
    Apply {
        operation_id: PushOperationId,
        push_id: PushId,
        #[source]
        source: Box<interaction_storage::StorageError>,
    },
    #[error("cannot acknowledge push operation {operation_id:?} for {push_id:?}")]
    Acknowledge {
        operation_id: PushOperationId,
        push_id: PushId,
        #[source]
        source: Box<automation_storage::StorageError>,
    },
}

pub struct PushHandoffWorker {
    automation: Arc<Mutex<AutomationStore>>,
    interactions: Arc<InteractionStore>,
    observation: crate::WorkerObservation,
}

impl PushHandoffWorker {
    pub fn new(
        automation: Arc<Mutex<AutomationStore>>,
        interactions: Arc<InteractionStore>,
    ) -> Self {
        Self {
            automation,
            interactions,
            observation: crate::WorkerObservation::default(),
        }
    }

    /// What the hand-off's loop reports; under test-support a test reads it.
    #[must_use]
    pub fn observation(&self) -> crate::WorkerObservation {
        self.observation.clone()
    }

    /// One Host-owned instance runs after exclusive startup and drains on shutdown. It
    /// sleeps until automation commits: owed push operations are due at once, so there
    /// is no timer except the failed-pass retry.
    pub async fn run(self, clock: Arc<dyn runtime_primitives::Clock>, shutdown: CancellationToken) {
        let commits = self.automation.lock().await.subscribe_commits();
        let worker = crate::WakeDrivenWorkerLoop::new("push hand-off", clock)
            .with_observation(self.observation.clone())
            .wake_on(commits);
        let mut pass = HandoffPass { worker: self };
        worker.run(&mut pass, shutdown).await;
    }

    pub async fn apply_pending(&mut self) -> Result<usize, PushHandoffError> {
        let operations = self
            .automation
            .lock()
            .await
            .list_owed_push_operations(automation_storage::MAX_OWED_PUSH_OPERATION_BATCH)
            .await
            .map_err(|source| PushHandoffError::Read(Box::new(source)))?;
        let mut blocked_pushes = Vec::new();
        let mut first_failure = None;
        let mut acknowledged = 0;
        for owed in operations {
            if blocked_pushes.contains(&owed.push_id) {
                continue;
            }
            let result = self.apply(&owed).await;
            if let Err(source) = result {
                blocked_pushes.push(owed.push_id.clone());
                first_failure.get_or_insert(PushHandoffError::Apply {
                    operation_id: owed.operation_id,
                    push_id: owed.push_id,
                    source: Box::new(source),
                });
                continue;
            }
            #[cfg(any(test, feature = "test-support"))]
            if matches!(&owed.operation, PushOperation::Admit { .. }) {
                crate::wakeup_delivery_sender::wake_handoff_checkpoint("push-admitted-before-ack");
            }
            if let Err(source) = self
                .automation
                .lock()
                .await
                .acknowledge_owed_push_operation(&owed)
                .await
            {
                blocked_pushes.push(owed.push_id.clone());
                first_failure.get_or_insert(PushHandoffError::Acknowledge {
                    operation_id: owed.operation_id,
                    push_id: owed.push_id,
                    source: Box::new(source),
                });
                continue;
            }
            acknowledged += 1;
        }
        match first_failure {
            Some(error) => Err(error),
            None => Ok(acknowledged),
        }
    }

    async fn apply(
        &self,
        owed: &OwedPushOperation,
    ) -> Result<(), interaction_storage::StorageError> {
        match &owed.operation {
            PushOperation::Admit { push } => {
                self.interactions
                    .admit_owed_wake_push(push.as_ref().clone())
                    .await?;
            }
            PushOperation::SettleScheduleRun {
                push_id,
                origin_ref,
                state,
                receipt,
                settled_at,
            } => {
                self.interactions
                    .apply_schedule_run_push_outcome(
                        push_id,
                        origin_ref,
                        *state,
                        receipt,
                        *settled_at,
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

/// The hand-off as the worker loop drives it: one batch of owed operations per call.
struct HandoffPass {
    worker: PushHandoffWorker,
}

impl crate::WorkerPass for HandoffPass {
    type Error = PushHandoffError;

    /// Applies one batch. Each acknowledgement commits, so a full batch wakes the loop
    /// for the next one; a pass that finds nothing owed writes nothing.
    async fn drain_batch(
        &mut self,
        _now: runtime_primitives::SyncTime,
        observation: &crate::WorkerObservation,
    ) -> Result<crate::BatchOutcome, PushHandoffError> {
        observation.record_read("owed push operations");
        self.worker.apply_pending().await?;
        Ok(crate::BatchOutcome::Drained)
    }

    async fn next_due(
        &mut self,
        _now: runtime_primitives::SyncTime,
        _observation: &crate::WorkerObservation,
    ) -> Result<Option<runtime_primitives::SyncTime>, PushHandoffError> {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "push_handoff_worker_test.rs"]
mod tests;
