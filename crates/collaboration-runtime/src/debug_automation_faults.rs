//! Debug-only, causally observable holds. Releasing one arm never releases a later arm.
use automation_storage::{
    AutomationFaultEvidence, AutomationFaultObserver, AutomationFaultPoint, StorageError,
};
use collaboration_protocol::OperationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, watch};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationFaultSelector {
    pub batch_id: Option<collaboration_protocol::BatchId>,
    pub firing_id: Option<collaboration_protocol::FiringId>,
    pub work_item_id: Option<collaboration_protocol::WorkItemId>,
    pub operation_id: Option<OperationId>,
    pub completed_entry_ordinal: Option<u32>,
}
impl AutomationFaultSelector {
    fn matches(&self, evidence: &AutomationFaultEvidence) -> bool {
        self.batch_id
            .as_ref()
            .is_none_or(|id| evidence.batch_id.as_ref() == Some(id))
            && self
                .firing_id
                .as_ref()
                .is_none_or(|id| evidence.firing_id.as_ref() == Some(id))
            && self
                .work_item_id
                .as_ref()
                .is_none_or(|id| evidence.work_item_id.as_ref() == Some(id))
            && self
                .operation_id
                .as_ref()
                .is_none_or(|id| evidence.operation_id.as_ref() == Some(id))
            && self
                .completed_entry_ordinal
                .is_none_or(|ordinal| evidence.completed_entry_ordinal == Some(ordinal))
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldAutomationFault {
    pub arm_id: OperationId,
    pub point: AutomationFaultPoint,
    pub evidence: AutomationFaultEvidence,
}
struct ArmedFault {
    id: OperationId,
    point: AutomationFaultPoint,
    selector: AutomationFaultSelector,
    reached: bool,
    release: Arc<Notify>,
}
pub struct DebugAutomationFaultController {
    armed: Mutex<Option<ArmedFault>>,
    held: watch::Sender<Option<HeldAutomationFault>>,
}
impl Default for DebugAutomationFaultController {
    fn default() -> Self {
        let (held, _) = watch::channel(None);
        Self {
            armed: Mutex::new(None),
            held,
        }
    }
}
impl DebugAutomationFaultController {
    pub async fn arm(
        &self,
        point: AutomationFaultPoint,
        selector: AutomationFaultSelector,
    ) -> Result<OperationId, StorageError> {
        let mut armed = self.armed.lock().await;
        if armed.is_some() {
            return Err(StorageError::OperationConflict);
        }
        let id = OperationId::generate();
        self.held.send_replace(None);
        *armed = Some(ArmedFault {
            id: id.clone(),
            point,
            selector,
            reached: false,
            release: Arc::new(Notify::new()),
        });
        Ok(id)
    }
    /// The response itself is the held-at-point signal; no polling or delay decides it.
    pub async fn wait_held(
        &self,
        arm_id: &OperationId,
    ) -> Result<HeldAutomationFault, StorageError> {
        let mut held = self.held.subscribe();
        loop {
            if let Some(record) = held.borrow().clone() {
                if &record.arm_id != arm_id {
                    return Err(StorageError::OperationConflict);
                }
                return Ok(record);
            }
            if self
                .armed
                .lock()
                .await
                .as_ref()
                .is_none_or(|armed| &armed.id != arm_id)
            {
                return Err(StorageError::OperationNotFound);
            }
            held.changed()
                .await
                .map_err(|_| StorageError::InvalidRecord)?;
        }
    }
    pub async fn release(&self, arm_id: &OperationId) -> Result<(), StorageError> {
        let mut armed = self.armed.lock().await;
        let Some(current) = armed.as_ref() else {
            return Err(StorageError::OperationNotFound);
        };
        if &current.id != arm_id {
            return Err(StorageError::OperationConflict);
        }
        if !current.reached {
            return Err(StorageError::InvalidRecord);
        }
        current.release.notify_one();
        *armed = None;
        Ok(())
    }
}
impl AutomationFaultObserver for DebugAutomationFaultController {
    fn hold(
        &self,
        point: AutomationFaultPoint,
        evidence: AutomationFaultEvidence,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let release = {
                let mut armed = self.armed.lock().await;
                let Some(current) = armed.as_mut() else {
                    return;
                };
                if current.reached || current.point != point || !current.selector.matches(&evidence)
                {
                    return;
                }
                current.reached = true;
                self.held.send_replace(Some(HeldAutomationFault {
                    arm_id: current.id.clone(),
                    point,
                    evidence,
                }));
                Arc::clone(&current.release)
            };
            release.notified().await;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions check causal holds; Result propagates control and task failures."
    )]
    async fn a_named_hold_waits_for_its_exact_release() -> Result<(), Box<dyn std::error::Error>> {
        let faults = Arc::new(DebugAutomationFaultController::default());
        let arm_id = faults
            .arm(
                AutomationFaultPoint::EntryAdmissionCommitted,
                AutomationFaultSelector {
                    completed_entry_ordinal: Some(2),
                    ..Default::default()
                },
            )
            .await?;
        faults
            .hold(
                AutomationFaultPoint::EntryAdmissionCommitted,
                AutomationFaultEvidence {
                    completed_entry_ordinal: Some(1),
                    ..Default::default()
                },
            )
            .await;
        let running = Arc::clone(&faults);
        let task = tokio::spawn(async move {
            running
                .hold(
                    AutomationFaultPoint::EntryAdmissionCommitted,
                    AutomationFaultEvidence {
                        completed_entry_ordinal: Some(2),
                        ..Default::default()
                    },
                )
                .await
        });
        let held =
            tokio::time::timeout(std::time::Duration::from_secs(2), faults.wait_held(&arm_id))
                .await??;
        assert_eq!(held.point, AutomationFaultPoint::EntryAdmissionCommitted);
        assert_eq!(held.evidence.completed_entry_ordinal, Some(2));
        assert!(!task.is_finished());
        assert!(faults.release(&OperationId::generate()).await.is_err());
        faults.release(&arm_id).await?;
        tokio::time::timeout(std::time::Duration::from_secs(2), task).await??;
        Ok(())
    }
}
