//! Central configuration does not assert the receiving machine's local Run occupancy.
use collaboration_protocol::{
    ChangeId, ImportedContinuity, ObservationTimestamp, ScheduleDefinition, ScheduleId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["nextDueAt"]}]))]
pub struct CentralScheduleSnapshot {
    pub schedule_id: ScheduleId,
    pub change_id: ChangeId,
    pub definition: ScheduleDefinition,
    pub imported_continuity: ImportedContinuity,
    pub anchor_at: ObservationTimestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_due_at: Option<ObservationTimestamp>,
    pub created_at: ObservationTimestamp,
    pub updated_at: ObservationTimestamp,
}
impl From<collaboration_protocol::ScheduleSnapshot> for CentralScheduleSnapshot {
    fn from(value: collaboration_protocol::ScheduleSnapshot) -> Self {
        let collaboration_protocol::ScheduleSnapshot {
            schedule_id,
            change_id,
            definition,
            imported_continuity,
            anchor_at,
            next_due_at,
            active_run_id: _,
            waiting_run_id: _,
            created_at,
            updated_at,
        } = value;
        Self {
            schedule_id,
            change_id,
            definition,
            imported_continuity,
            anchor_at,
            next_due_at,
            created_at,
            updated_at,
        }
    }
}
