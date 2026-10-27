//! The single local worker applies ordered automation operations without dispatching native input.
use automation_storage::{AutomationStore, OwedPushOperation, PushOperation, PushOperationId};
use collaboration_protocol::PushId;
use interaction_storage::InteractionStore;
use std::{sync::Arc, time::Duration};
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
}

impl PushHandoffWorker {
    pub fn new(
        automation: Arc<Mutex<AutomationStore>>,
        interactions: Arc<InteractionStore>,
    ) -> Self {
        Self {
            automation,
            interactions,
        }
    }

    /// One Host-owned instance runs after exclusive startup and drains on shutdown.
    pub async fn run(mut self, shutdown: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut reported_failure = false;
        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => return,
                _ = interval.tick() => {},
            }
            match self.apply_pending().await {
                Ok(_) => reported_failure = false,
                Err(error) => {
                    if !reported_failure {
                        tracing::warn!(%error, "automation push operation remains owed; no native input is dispatched by this worker");
                    }
                    reported_failure = true;
                }
            }
        }
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
            #[cfg(test)]
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
