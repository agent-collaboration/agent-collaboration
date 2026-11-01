//! Cron validation examines the parser's whole minute set, never a sample of runs.
use crate::{AutomationValidationError, GridTime, automation_grid::off_grid, limits::GRID_STEP};
use chrono::{DateTime, Offset, Utc};
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(try_from = "CronFields")]
pub struct GridCron {
    expression: String,
    timezone: String,
}

#[derive(JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CronFields {
    expression: String,
    timezone: String,
}

impl GridCron {
    pub fn new(expression: String, timezone: String) -> Result<Self, AutomationValidationError> {
        let cron = parse_cron(&expression)?;
        let _: chrono_tz::Tz = timezone
            .parse()
            .map_err(|_| AutomationValidationError::InvalidTimezone)?;
        for minute in 0..60 {
            if cron
                .pattern
                .minute_match(minute)
                .map_err(|_| AutomationValidationError::InvalidCron)?
                && minute % (GRID_STEP / 60) != 0
            {
                return Err(off_grid("cron.minute", i64::from(minute) * 60_000));
            }
        }
        Ok(Self {
            expression,
            timezone,
        })
    }

    pub fn next_after(&self, after_ms: i64) -> Result<GridTime, AutomationValidationError> {
        let timezone: chrono_tz::Tz = self
            .timezone
            .parse()
            .map_err(|_| AutomationValidationError::InvalidTimezone)?;
        let after = DateTime::<Utc>::from_timestamp_millis(after_ms)
            .ok_or(AutomationValidationError::TimeOutOfRange)?;
        let due = parse_cron(&self.expression)?
            .find_next_occurrence(&after.with_timezone(&timezone), false)
            .map_err(|_| AutomationValidationError::TimeOutOfRange)?;
        let offset_seconds = due.offset().fix().local_minus_utc();
        if offset_seconds
            % i32::try_from(GRID_STEP).map_err(|_| AutomationValidationError::TimeOutOfRange)?
            != 0
        {
            return Err(off_grid(
                &format!("timezone {} offset {offset_seconds} seconds", self.timezone),
                due.timestamp_millis(),
            ));
        }
        GridTime::new(due.timestamp_millis())
    }

    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }
    #[must_use]
    pub fn timezone(&self) -> &str {
        &self.timezone
    }
}

fn parse_cron(expression: &str) -> Result<Cron, AutomationValidationError> {
    if expression.split_whitespace().count() != 5 {
        return Err(AutomationValidationError::InvalidCron);
    }
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expression)
        .map_err(|_| AutomationValidationError::InvalidCron)
}

impl TryFrom<CronFields> for GridCron {
    type Error = AutomationValidationError;
    fn try_from(fields: CronFields) -> Result<Self, Self::Error> {
        Self::new(fields.expression, fields.timezone)
    }
}
