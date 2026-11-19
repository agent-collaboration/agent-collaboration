//! Central timing admission and portable timestamp bounds.
use agent_automation::{TimingError, TimingRule};
use workspace_model::Timestamp;
use workspace_model::central_mail::{CentralMailOperation, CentralMailRefusal, TimingRequirement};

use fleet_contracts::timer_schedule::{
    GRID_SECONDS, LIFETIME_SECONDS, first_due, lifetime_end, on_grid, supported, valid_rule,
};
#[cfg(test)]
use fleet_contracts::timer_schedule::{aligned_at_or_after, aligned_strictly_after};

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

pub(crate) fn refusal(
    operation: &CentralMailOperation,
    now: Timestamp,
) -> Option<CentralMailRefusal> {
    use CentralMailOperation as Op;
    if !supported(now) {
        return Some(out_of_range("anchorAt"));
    }
    match operation {
        Op::SetWake { due_at, .. } => {
            if !supported(*due_at) {
                return Some(out_of_range("dueAt"));
            }
            if !on_grid(*due_at) {
                return Some(violation("dueAt", TimingRequirement::FiveMinuteGrid));
            }
            if *due_at <= now {
                return Some(violation("dueAt", TimingRequirement::FutureTime));
            }
            let Some(expiry) = lifetime_end(now) else {
                return Some(out_of_range("anchorAt"));
            };
            if *due_at > expiry {
                return Some(violation("dueAt", TimingRequirement::WithinSevenDays));
            }
        }
        Op::SetAutomation { timing, .. } => {
            if let TimingRule::At { at } = timing
                && !supported(*at)
            {
                return Some(out_of_range("timing.at"));
            }
            let Some(expiry) = lifetime_end(now) else {
                return Some(out_of_range("anchorAt"));
            };
            let field = match timing {
                TimingRule::At { .. } => "timing.at",
                TimingRule::After { .. } | TimingRule::Interval { .. } => "timing.seconds",
                TimingRule::Cron { .. } => "timing.expression",
            };
            if let TimingRule::After { seconds } | TimingRule::Interval { seconds } = timing
                && i64::from(*seconds) > LIFETIME_SECONDS
            {
                return Some(violation(field, TimingRequirement::WithinSevenDays));
            }
            // Detect malformed durations before asking the shared calculator to validate them.
            if let TimingRule::After { seconds } | TimingRule::Interval { seconds } = timing
                && (*seconds < GRID_SECONDS || *seconds % GRID_SECONDS != 0)
            {
                return Some(violation(field, TimingRequirement::FiveMinuteGrid));
            }
            match valid_rule(timing, now, expiry) {
                Ok(false) => return Some(violation(field, TimingRequirement::FiveMinuteGrid)),
                // The shared calculator also uses OutOfRange for an impossible
                // calendar (for example February 30). The planner owns its
                // retained NoOccurrence answer; portable due bounds are checked below.
                Err(TimingError::OutOfRange) => return None,
                Err(_) => return None, // The timer planner returns InvalidInput.
                Ok(true) => {}
            }
            if let Ok(Some(due)) = first_due(timing, now) {
                if !supported(due) {
                    return Some(out_of_range("nextDueAt"));
                }
                if matches!(timing, TimingRule::At { .. }) && due <= now {
                    return Some(violation(field, TimingRequirement::FutureTime));
                }
                if due > expiry {
                    return Some(violation(field, TimingRequirement::WithinSevenDays));
                }
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
#[path = "central_mail_time_test.rs"]
mod tests;
