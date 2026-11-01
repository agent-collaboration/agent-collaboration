//! Resolved timing stores its chosen first deadline, so replays never consult a new clock.
use crate::{
    AutomationValidationError, GridCron, GridPeriod, GridTime, TimingRequest,
    automation_grid::off_grid,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationKind {
    Schedule,
    Wake,
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(try_from = "AutomationTimingFields", into = "AutomationTimingFields")]
pub struct AutomationTiming(AutomationTimingFields);

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum AutomationTimingFields {
    At {
        at: GridTime,
    },
    Interval {
        seconds: GridPeriod,
        first_due: GridTime,
    },
    Cron {
        cron: GridCron,
        first_due: GridTime,
    },
}

impl AutomationTiming {
    pub fn resolve(
        request: &TimingRequest,
        kind: AutomationKind,
        now_ms: i64,
    ) -> Result<AutomationTiming, AutomationValidationError> {
        match request {
            TimingRequest::At { at } => {
                let at = parse_timestamp(String::from(at.clone()))?;
                AutomationTiming::try_from(AutomationTimingFields::At {
                    at: GridTime::new(at)?,
                })
            }
            TimingRequest::After { seconds } => {
                let candidate = now_ms
                    .checked_add(i64::from(u32::from(*seconds)) * 1000)
                    .ok_or(AutomationValidationError::TimeOutOfRange)?;
                Err(off_grid(
                    "after is not supported for automations",
                    candidate,
                ))
            }
            TimingRequest::Interval { seconds, first_due } => {
                let value = u32::from(*seconds);
                if !value.is_multiple_of(crate::limits::GRID_STEP) {
                    let candidate = now_ms
                        .checked_add(i64::from(value) * 1000)
                        .ok_or(AutomationValidationError::TimeOutOfRange)?;
                    return Err(off_grid("seconds", candidate));
                }
                let seconds = match kind {
                    AutomationKind::Schedule => GridPeriod::new(value)?,
                    AutomationKind::Wake => GridPeriod::for_wake(value)?,
                };
                AutomationTiming::try_from(AutomationTimingFields::Interval {
                    seconds,
                    first_due: match first_due {
                        Some(due) => GridTime::new(parse_timestamp(String::from(due.clone()))?)?,
                        None => GridTime::next_after(now_ms)?,
                    },
                })
            }
            TimingRequest::Cron {
                expression,
                timezone,
            } => {
                let cron = GridCron::new(expression.clone(), timezone.clone())?;
                let first_due = cron.next_after(now_ms)?;
                if kind == AutomationKind::Wake {
                    validate_wake_cron(&cron, first_due, now_ms)?;
                }
                AutomationTiming::try_from(AutomationTimingFields::Cron { cron, first_due })
            }
        }
    }
}

fn validate_wake_cron(
    cron: &GridCron,
    first: GridTime,
    now_ms: i64,
) -> Result<(), AutomationValidationError> {
    let end = now_ms
        .checked_add(i64::from(crate::limits::WAKE_REPEAT_MAX_LIFETIME) * 1000)
        .ok_or(AutomationValidationError::TimeOutOfRange)?;
    let mut previous = first.unix_millis();
    while previous < end {
        let next = cron.next_after(previous)?.unix_millis();
        if next > end {
            break;
        }
        let gap = next
            .checked_sub(previous)
            .and_then(|gap| u64::try_from(gap / 1000).ok())
            .ok_or(AutomationValidationError::TimeOutOfRange)?;
        crate::limits::LimitName::WakeMinPeriod.validate(gap)?;
        previous = next;
    }
    Ok(())
}

impl AutomationTiming {
    #[must_use]
    pub fn first_due(&self) -> GridTime {
        match &self.0 {
            AutomationTimingFields::At { at } => *at,
            AutomationTimingFields::Interval { first_due, .. }
            | AutomationTimingFields::Cron { first_due, .. } => *first_due,
        }
    }

    pub fn next_after(&self, after_ms: i64) -> Result<Option<GridTime>, AutomationValidationError> {
        match &self.0 {
            AutomationTimingFields::At { at } => {
                Ok(Some(*at).filter(|due| due.unix_millis() > after_ms))
            }
            AutomationTimingFields::Interval { seconds, first_due } => {
                let first = first_due.unix_millis();
                if after_ms < first {
                    return Ok(Some(*first_due));
                }
                let period = i64::from(seconds.seconds()) * 1000;
                let steps = after_ms
                    .checked_sub(first)
                    .and_then(|elapsed| elapsed.checked_div(period))
                    .and_then(|steps| steps.checked_add(1))
                    .ok_or(AutomationValidationError::TimeOutOfRange)?;
                let next = steps
                    .checked_mul(period)
                    .and_then(|offset| first.checked_add(offset))
                    .ok_or(AutomationValidationError::TimeOutOfRange)?;
                GridTime::new(next).map(Some)
            }
            AutomationTimingFields::Cron { cron, first_due } => {
                if after_ms < first_due.unix_millis() {
                    return Ok(Some(*first_due));
                }
                cron.next_after(after_ms).map(Some)
            }
        }
    }
}

impl TryFrom<AutomationTimingFields> for AutomationTiming {
    type Error = AutomationValidationError;
    fn try_from(fields: AutomationTimingFields) -> Result<Self, Self::Error> {
        if let AutomationTimingFields::Cron { cron, first_due } = &fields {
            let before = first_due
                .unix_millis()
                .checked_sub(1)
                .ok_or(AutomationValidationError::TimeOutOfRange)?;
            if cron.next_after(before)? != *first_due {
                return Err(AutomationValidationError::InvalidCronFirstDue);
            }
        }
        Ok(Self(fields))
    }
}

impl From<AutomationTiming> for AutomationTimingFields {
    fn from(timing: AutomationTiming) -> Self {
        timing.0
    }
}

fn parse_timestamp(value: String) -> Result<i64, AutomationValidationError> {
    let timestamp: DateTime<Utc> = value
        .parse()
        .map_err(|_| AutomationValidationError::TimeOutOfRange)?;
    if !timestamp.timestamp_subsec_nanos().is_multiple_of(1_000_000) {
        return Err(off_grid("at", timestamp.timestamp_millis()));
    }
    Ok(timestamp.timestamp_millis())
}
