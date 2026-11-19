//! Timing rules used by stored timer rows and the mail request planner.
use agent_automation::{TimingError, TimingRule};
use chrono::Duration;
use workspace_model::Timestamp;

/// Last integer millisecond accepted by pinned workerd's signed nanosecond clock.
/// Proven by `platform_alarm_accepts_exact_boundary_and_rejects_later_milliseconds`;
/// original receipt: tmp/2026-11-14-b3/platform-range-probe.log (1 passed, exit0).
/// Pinned source: cloudflare/workerd v1.20261012.1, jsg/value.h:1218–1224.
pub const MAX_ALARM_UNIX_MILLIS: i64 = 9_223_372_036_854;
pub const GRID_SECONDS: u32 = 300;
pub const LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;

pub fn supported(time: Timestamp) -> bool {
    (-MAX_ALARM_UNIX_MILLIS..=MAX_ALARM_UNIX_MILLIS).contains(&time.timestamp_millis())
}

pub fn aligned_at_or_after(time: Timestamp, seconds: u32) -> Option<Timestamp> {
    if seconds == 0 {
        return None;
    }
    let seconds = i64::from(seconds);
    let remainder = time.timestamp().rem_euclid(seconds);
    let add = if remainder == 0 && time.timestamp_subsec_nanos() == 0 {
        0
    } else {
        seconds - remainder
    };
    let whole = time.timestamp().checked_add(add)?;
    Timestamp::from_timestamp(whole, 0).filter(|value| supported(*value))
}

pub fn aligned_strictly_after(time: Timestamp, seconds: u32) -> Option<Timestamp> {
    let after = time.checked_add_signed(Duration::nanoseconds(1))?;
    aligned_at_or_after(after, seconds)
}

pub fn on_grid(time: Timestamp) -> bool {
    time.timestamp().rem_euclid(i64::from(GRID_SECONDS)) == 0 && time.timestamp_subsec_nanos() == 0
}

pub fn lifetime_end(now: Timestamp) -> Option<Timestamp> {
    now.checked_add_signed(Duration::seconds(LIFETIME_SECONDS))
        .filter(|time| supported(*time))
}

pub fn first_due(timing: &TimingRule, now: Timestamp) -> Result<Option<Timestamp>, TimingError> {
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
pub fn valid_rule(
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
