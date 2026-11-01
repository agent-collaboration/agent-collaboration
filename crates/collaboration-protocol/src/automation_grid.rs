//! Executable central timing is validated on entry and again on stored decode.
use crate::limits::{DURATION_MAX_SECONDS, GRID_STEP, LimitExceeded, LimitName};
use chrono::{DateTime, Utc};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

pub const GRID_STEP_MS: i64 = GRID_STEP as i64 * 1000;

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationValidationError {
    #[error("offGrid: {field}; nearest grid times are {earlier} and {later}")]
    OffGrid {
        field: String,
        earlier: String,
        later: String,
    },
    #[error(transparent)]
    LimitExceeded(#[from] LimitExceeded),
    #[error("invalidCron: expected five valid cron fields")]
    InvalidCron,
    #[error("stored first due time is not an occurrence of its cron expression")]
    InvalidCronFirstDue,
    #[error("invalidTimezone: expected an explicit IANA time zone")]
    InvalidTimezone,
    #[error("timing exceeds the supported datetime range")]
    TimeOutOfRange,
}

pub(crate) fn off_grid(field: &str, value_ms: i64) -> AutomationValidationError {
    let earlier_ms = value_ms.div_euclid(GRID_STEP_MS).checked_mul(GRID_STEP_MS);
    let Some((earlier_ms, later_ms)) = earlier_ms.and_then(|earlier| {
        earlier
            .checked_add(GRID_STEP_MS)
            .map(|later| (earlier, later))
    }) else {
        return AutomationValidationError::TimeOutOfRange;
    };
    let Some(earlier) = DateTime::<Utc>::from_timestamp_millis(earlier_ms) else {
        return AutomationValidationError::TimeOutOfRange;
    };
    let Some(later) = DateTime::<Utc>::from_timestamp_millis(later_ms) else {
        return AutomationValidationError::TimeOutOfRange;
    };
    AutomationValidationError::OffGrid {
        field: field.into(),
        earlier: earlier.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        later: later.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct GridTime(i64);

impl GridTime {
    pub fn new(value_ms: i64) -> Result<Self, AutomationValidationError> {
        if value_ms.rem_euclid(GRID_STEP_MS) != 0 {
            return Err(off_grid("at", value_ms));
        }
        DateTime::<Utc>::from_timestamp_millis(value_ms)
            .ok_or(AutomationValidationError::TimeOutOfRange)?;
        Ok(Self(value_ms))
    }

    /// The next boundary is strictly later, including when now is already on the grid.
    pub fn next_after(now_ms: i64) -> Result<Self, AutomationValidationError> {
        let next = now_ms
            .div_euclid(GRID_STEP_MS)
            .checked_add(1)
            .and_then(|step| step.checked_mul(GRID_STEP_MS))
            .ok_or(AutomationValidationError::TimeOutOfRange)?;
        Self::new(next)
    }

    #[must_use]
    pub const fn unix_millis(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for GridTime {
    type Error = AutomationValidationError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<GridTime> for i64 {
    fn from(value: GridTime) -> Self {
        value.0
    }
}
impl JsonSchema for GridTime {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GridTime".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"integer", "multipleOf":GRID_STEP_MS,
            "description":"UTC Unix milliseconds on a five-minute mark. Off-grid values are refused, never rounded."})
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct GridPeriod(u32);
impl GridPeriod {
    pub fn new(seconds: u32) -> Result<Self, AutomationValidationError> {
        if !seconds.is_multiple_of(GRID_STEP) {
            return Err(off_grid("seconds", i64::from(seconds) * 1000));
        }
        LimitName::ScheduleMinInterval.validate(u64::from(seconds))?;
        Ok(Self(seconds))
    }
    pub fn for_wake(seconds: u32) -> Result<Self, AutomationValidationError> {
        let period = Self::new(seconds)?;
        LimitName::WakeMinPeriod.validate(u64::from(seconds))?;
        Ok(period)
    }
    #[must_use]
    pub const fn seconds(self) -> u32 {
        self.0
    }
}
impl TryFrom<u32> for GridPeriod {
    type Error = AutomationValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<GridPeriod> for u32 {
    fn from(value: GridPeriod) -> Self {
        value.0
    }
}
impl JsonSchema for GridPeriod {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GridPeriod".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"integer", "minimum":GRID_STEP,
            "maximum":DURATION_MAX_SECONDS, "multipleOf":GRID_STEP,
            "description":"A schedule period in seconds, with exactly five-minute precision."})
    }
}
