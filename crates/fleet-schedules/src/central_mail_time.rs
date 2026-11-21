//! Timing checks for the schedule request and stored occurrence.
use agent_automation::{TimingError, TimingRule};
use central_engine_model::{
    aligned_at_or_after, on_five_minute_grid as on_grid, supported_time as supported,
};
use chrono::Duration;
use fleet_timer_policy::{LIFETIME_SECONDS, lifetime_end};
use workspace_model::Timestamp;
use workspace_model::central_mail::{CentralMailRefusal, TimingRequirement};

const GRID_SECONDS: u32 = 300;

pub(crate) fn first_due(
    timing: &TimingRule,
    now: Timestamp,
) -> Result<Option<Timestamp>, TimingError> {
    let anchor = aligned_at_or_after(now, GRID_SECONDS).ok_or(TimingError::OutOfRange)?;
    match timing {
        TimingRule::Interval { .. } => {
            // Keep TimingRule's own duration validation while making the anchor the first occurrence.
            timing.next_due(anchor, None)?;
            Ok(Some(anchor))
        }
        TimingRule::After { .. } => timing.next_due(anchor, None),
        _ => timing.next_due(now, None),
    }
}

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

// Reuse the canonical cron parser to expand its minute field, including ranges and steps.
fn cron_minutes_on_grid(expression: &str, anchor: Timestamp) -> Result<bool, TimingError> {
    let minute = expression
        .split_whitespace()
        .next()
        .ok_or(TimingError::InvalidCron)?;
    let probe = TimingRule::Cron {
        expression: format!("{minute} * * * *"),
        timezone: "UTC".to_owned(),
    };
    let hour = anchor
        .timestamp()
        .div_euclid(3600)
        .checked_mul(3600)
        .ok_or(TimingError::OutOfRange)?;
    let start = Timestamp::from_timestamp(hour, 0).ok_or(TimingError::OutOfRange)?;
    let end = start
        .checked_add_signed(Duration::hours(1))
        .ok_or(TimingError::OutOfRange)?;
    let mut boundary = start
        .checked_sub_signed(Duration::seconds(1))
        .ok_or(TimingError::OutOfRange)?;
    loop {
        let Some(next) = probe.next_due(boundary, None)? else {
            return Ok(true);
        };
        if next >= end {
            return Ok(true);
        }
        if !on_grid(next) {
            return Ok(false);
        }
        boundary = next;
    }
}

/// Validate rule shape and every actual UTC occurrence within the persisted lifetime.
pub(crate) fn valid_rule(
    timing: &TimingRule,
    admission: Timestamp,
    expiry: Timestamp,
) -> Result<bool, TimingError> {
    let first = first_due(timing, admission)?;
    match timing {
        TimingRule::At { at } => Ok(on_grid(*at)),
        TimingRule::After { seconds } | TimingRule::Interval { seconds } => {
            Ok(*seconds >= GRID_SECONDS && *seconds % GRID_SECONDS == 0)
        }
        TimingRule::Cron {
            expression,
            timezone,
        } => {
            if !cron_minutes_on_grid(expression, admission)? {
                return Ok(false);
            }
            // UTC has no offset transitions, so its selected minute check is sufficient.
            if timezone == "UTC" {
                return Ok(true);
            }
            let mut next = first;
            while let Some(due) = next.filter(|due| *due <= expiry) {
                if !on_grid(due) {
                    return Ok(false);
                }
                next = timing.next_due(admission, Some(due))?;
            }
            Ok(true)
        }
    }
}

pub fn set_automation_refusal(timing: &TimingRule, now: Timestamp) -> Option<CentralMailRefusal> {
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
    None
}

#[cfg(test)]
#[path = "central_mail_time_test.rs"]
mod tests;
