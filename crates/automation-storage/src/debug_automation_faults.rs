//! Debug-only transaction observation; the runtime owns arming and release.
use collaboration_protocol::{BatchId, FiringId, OperationId, RunId, WorkItemId};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationFaultPoint {
    BatchFetchedBeforeAdmission,
    EntryAdmissionBeforeCommit,
    EntryAdmissionCommitted,
    BatchAdmissionCommittedBeforeAck,
    NativeInputAcceptedBeforeOutcomeCommit,
    ReportSentBeforeAcknowledgement,
    RemoteCreateAdmittedBeforeDispatch,
    RemoteCreateAcceptedBeforeReceiptCommit,
    FirstInputAcceptedBeforeReceiptCommit,
    HandoverBeforeFreeze,
    HandoverAfterFreeze,
    HandoverImportSent,
    HandoverImportAcknowledged,
    HandoverMoved,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationFaultEvidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<BatchId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firing_id: Option<FiringId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_item_id: Option<WorkItemId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_attempt_id: Option<agent_automation::AttemptId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_entry_ordinal: Option<u32>,
}
pub trait AutomationFaultObserver: Send + Sync {
    fn hold(
        &self,
        point: AutomationFaultPoint,
        evidence: AutomationFaultEvidence,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}
