//! Fresh mail request timing admission.
use central_engine_model::supported_time;
use workspace_model::Timestamp;
use workspace_model::central_mail::{CentralMailOperation, CentralMailRefusal};

/// Last integer millisecond accepted by the portable alarm clock.
pub const MAX_ALARM_UNIX_MILLIS: i64 = central_engine_model::MAX_ALARM_UNIX_MILLIS;

pub(crate) fn refusal(
    operation: &CentralMailOperation,
    now: Timestamp,
) -> Option<CentralMailRefusal> {
    if !supported_time(now) {
        return Some(CentralMailRefusal::TimeOutOfRange {
            field: "anchorAt".to_owned(),
        });
    }
    match operation {
        CentralMailOperation::SetWake { due_at, .. } => fleet_wakes::set_wake_refusal(*due_at, now),
        CentralMailOperation::SetAutomation { timing, .. } => {
            fleet_schedules::set_automation_refusal(timing, now)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "central_mail_time_test.rs"]
mod tests;
