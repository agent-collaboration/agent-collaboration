use crate::{DueBatchPolicy, EngineTime};

pub const MAX_ALARM_UNIX_MILLIS: i64 = 9_223_372_036_854;
const FIVE_MINUTE_MILLIS: i64 = 300_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlarmInputs {
    pub now: EngineTime,
    pub last_batch_start: Option<EngineTime>,
    pub leftovers_beyond_cap: bool,
    pub earliest_on_grid_due: Option<EngineTime>,
    pub earliest_legacy_due: Option<EngineTime>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlarmDecision {
    Clear,
    SetAt(EngineTime),
    Unpublishable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AlarmDecisionError {
    #[error("no later five-minute mark is within the portable range")]
    NoLaterGridMark,
    #[error("the alarm time is outside the portable range")]
    OutOfRange,
}

pub fn supported_time(time: EngineTime) -> bool {
    (-MAX_ALARM_UNIX_MILLIS..=MAX_ALARM_UNIX_MILLIS).contains(&time.timestamp_millis())
}

pub fn next_five_minute_mark(now: EngineTime) -> Result<EngineTime, AlarmDecisionError> {
    if !supported_time(now) {
        return Err(AlarmDecisionError::OutOfRange);
    }
    now.timestamp_millis()
        .div_euclid(FIVE_MINUTE_MILLIS)
        .checked_add(1)
        .and_then(|mark| mark.checked_mul(FIVE_MINUTE_MILLIS))
        .and_then(EngineTime::from_timestamp_millis)
        .filter(|mark| supported_time(*mark))
        .ok_or(AlarmDecisionError::NoLaterGridMark)
}

/// Combine pending times after an event; an idle object has no alarm.
pub fn next_alarm(inputs: &AlarmInputs, policy: &DueBatchPolicy) -> AlarmDecision {
    if inputs.earliest_on_grid_due.is_none() && inputs.earliest_legacy_due.is_none() {
        return AlarmDecision::Clear;
    }
    if !supported_time(inputs.now)
        || [inputs.earliest_on_grid_due, inputs.earliest_legacy_due]
            .into_iter()
            .flatten()
            .any(|due| !supported_time(due))
    {
        return AlarmDecision::Unpublishable;
    }
    let floor = match inputs.last_batch_start {
        Some(start) => match start
            .checked_add_signed(policy.spacing)
            .filter(|time| supported_time(*time))
        {
            Some(floor) => Some(floor),
            None => return AlarmDecision::Unpublishable,
        },
        None => None,
    };
    let wanted = if inputs.leftovers_beyond_cap {
        match floor {
            Some(floor) => floor,
            None => return AlarmDecision::Unpublishable,
        }
    } else {
        // At least one input is present, as established above.
        match (inputs.earliest_on_grid_due, inputs.earliest_legacy_due) {
            (Some(grid), Some(legacy)) => grid.min(legacy),
            (Some(due), None) | (None, Some(due)) => due,
            (None, None) => return AlarmDecision::Clear,
        }
    };
    let wanted = floor.map_or(wanted, |floor| wanted.max(floor));
    if wanted <= inputs.now {
        next_five_minute_mark(inputs.now).map_or(AlarmDecision::Unpublishable, AlarmDecision::SetAt)
    } else {
        AlarmDecision::SetAt(wanted)
    }
}

#[cfg(test)]
#[path = "next_alarm_test.rs"]
mod tests;
