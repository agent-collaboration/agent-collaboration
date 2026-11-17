use super::super::workerd_host::c2_controls::WitnessRecord;
use central_link_protocol::DataFrame;
use central_storage::ReadOutcome;
use fleet_identity_protocol::OrganizationId;
use serde::Serialize;
use workspace_model::{AreaId, MachineId};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HibernationReceipt {
    pub kind: &'static str,
    pub organization: OrganizationId,
    pub installation_id: MachineId,
    pub welcome: DataFrame,
    pub baseline_request: DataFrame,
    pub baseline_reply: DataFrame,
    pub deciding_request: DataFrame,
    pub deciding_reply: DataFrame,
    pub read_request: DataFrame,
    pub read_reply: DataFrame,
    pub before_events: Vec<WitnessRecord>,
    pub heartbeat_events: Vec<Vec<WitnessRecord>>,
    pub deciding_events: Vec<WitnessRecord>,
    pub area: AreaReceipt,
    pub store: ReadOutcome,
    pub disposed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AreaReceipt {
    pub area_id: AreaId,
    pub name: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BatchHibernationReceipt {
    pub kind: &'static str,
    pub organization: OrganizationId,
    pub installation_id: MachineId,
    pub welcome: DataFrame,
    pub baseline_request: DataFrame,
    pub baseline_reply: DataFrame,
    pub batch_request: DataFrame,
    pub batch_replies: Vec<DataFrame>,
    pub read_requests: Vec<DataFrame>,
    pub read_replies: Vec<DataFrame>,
    pub before_events: Vec<WitnessRecord>,
    pub heartbeat_events: Vec<Vec<WitnessRecord>>,
    pub deciding_events: Vec<WitnessRecord>,
    pub areas: Vec<AreaReceipt>,
    pub store: ReadOutcome,
    pub wakes: ReadOutcome,
    pub disposed: bool,
}
