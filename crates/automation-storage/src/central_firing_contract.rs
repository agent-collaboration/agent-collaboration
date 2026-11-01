//! Frozen admission input and receipt identity, independent of persistence and projection helpers.
use agent_automation::{DeliveryId, RunId};
use collaboration_protocol::{
    AutomationContent, BatchId, FiringEntry, FiringId, FiringOutcome, WorkReportSequence,
};
use serde::{Deserialize, Serialize};

pub struct CentralFiringRequest {
    pub batch_id: BatchId,
    pub firing: FiringEntry,
    pub content: AutomationContent,
    pub now_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralLocalWork {
    Schedule { run_id: RunId },
    Wake { delivery_id: DeliveryId },
    ExpiredWake,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralFiringReceipt {
    pub firing_id: FiringId,
    pub batch_id: BatchId,
    pub local_work: CentralLocalWork,
    pub outcome: FiringOutcome,
    pub report_seq: WorkReportSequence,
}
