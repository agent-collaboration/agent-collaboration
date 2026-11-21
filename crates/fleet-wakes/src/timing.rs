//! Wake admission checks on the existing clock sample.
use central_engine_model::{on_five_minute_grid as on_grid, supported_time as supported};
use fleet_timer_policy::lifetime_end;
use workspace_model::Timestamp;
use workspace_model::central_mail::{CentralMailRefusal, TimingRequirement};

fn violation(field: &str, requirement: TimingRequirement) -> CentralMailRefusal {
    CentralMailRefusal::TimingViolation {
        field: field.to_owned(),
        requirement,
    }
}
fn out_of_range(field: &str) -> CentralMailRefusal {
    CentralMailRefusal::TimeOutOfRange {
        field: field.to_owned(),
    }
}

pub fn set_wake_refusal(due_at: Timestamp, now: Timestamp) -> Option<CentralMailRefusal> {
    if !supported(due_at) {
        return Some(out_of_range("dueAt"));
    }
    if !on_grid(due_at) {
        return Some(violation("dueAt", TimingRequirement::FiveMinuteGrid));
    }
    if due_at <= now {
        return Some(violation("dueAt", TimingRequirement::FutureTime));
    }
    let Some(expiry) = lifetime_end(now) else {
        return Some(out_of_range("anchorAt"));
    };
    if due_at > expiry {
        return Some(violation("dueAt", TimingRequirement::WithinSevenDays));
    }
    None
}
