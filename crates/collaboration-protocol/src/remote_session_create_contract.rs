//! Central custody and target-reported outcomes are independent observations.
use crate::{FirstInputState, OperationId, SessionCreateState, WorkItemId, WorkReportSequence};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MachineWorkCustodyState {
    Pending,
    Held,
    Received,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["create","reportSeq"]}]))]
pub struct RemoteSessionCreateSnapshot {
    pub work_item_id: WorkItemId,
    pub operation_id: OperationId,
    pub state: MachineWorkCustodyState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub create: Option<SessionCreateState>,
    pub input: FirstInputState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub report_seq: Option<WorkReportSequence>,
}
