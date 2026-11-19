//! The one place croner's jiff 0.2 types appear.
//!
//! croner's `jiff` feature is built on jiff 0.2, while our code uses jiff 1.x.
//! The two versions exchange Unix timestamp parts and the input IANA zone name.
//! When croner ships a jiff-1 feature, this module and the `jiff-croner`
//! dependency go.
use super::TimingError;
use chrono::{DateTime, Utc};
use croner::parser::{CronParser, Seconds, Year};
use jiff::tz::TimeZone;

/// Finds the first scheduled instant strictly after the supplied UTC boundary.
pub(super) fn next_occurrence(
    expression: &str,
    start: DateTime<Utc>,
    zone_name: &str,
) -> Result<DateTime<Utc>, TimingError> {
    if expression.split_whitespace().count() != 5 {
        return Err(TimingError::InvalidCron);
    }

    let validated_zone = TimeZone::get(zone_name).map_err(|_| TimingError::InvalidTimezone)?;
    // Preserve exact IANA spelling and reject Jiff's unknown-zone placeholder.
    if validated_zone.iana_name() != Some(zone_name) {
        return Err(TimingError::InvalidTimezone);
    }
    let cron = CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expression)
        .map_err(|_| TimingError::InvalidCron)?;

    let croner_zone =
        jiff_croner::tz::TimeZone::get(zone_name).map_err(|_| TimingError::InvalidTimezone)?;
    let start = to_croner(timestamp(start)?)?.to_zoned(croner_zone);
    let due = cron
        .find_next_occurrence(&start, false)
        .map_err(|_| TimingError::OutOfRange)?;
    from_croner(due.timestamp()).and_then(utc)
}

fn timestamp(at: DateTime<Utc>) -> Result<jiff::Timestamp, TimingError> {
    jiff::Timestamp::new(
        at.timestamp(),
        saturating_nanos(at.timestamp_subsec_nanos()),
    )
    .map_err(|_| TimingError::OutOfRange)
}

fn utc(timestamp: jiff::Timestamp) -> Result<DateTime<Utc>, TimingError> {
    // jiff gives a negative instant's parts with the same sign; chrono wants
    // whole seconds below it and non-negative nanoseconds.
    let (seconds, nanos) = match timestamp.subsec_nanosecond() {
        nanos if nanos < 0 => (timestamp.as_second() - 1, nanos + 1_000_000_000),
        nanos => (timestamp.as_second(), nanos),
    };
    DateTime::from_timestamp(seconds, nanos.unsigned_abs()).ok_or(TimingError::OutOfRange)
}

/// chrono marks a leap second as 1,000,000,000 or more nanoseconds; jiff has none.
fn saturating_nanos(nanos: u32) -> i32 {
    i32::try_from(nanos.min(999_999_999)).unwrap_or(999_999_999)
}

fn to_croner(at: jiff::Timestamp) -> Result<jiff_croner::Timestamp, TimingError> {
    jiff_croner::Timestamp::new(at.as_second(), at.subsec_nanosecond())
        .map_err(|_| TimingError::OutOfRange)
}

fn from_croner(at: jiff_croner::Timestamp) -> Result<jiff::Timestamp, TimingError> {
    jiff::Timestamp::new(at.as_second(), at.subsec_nanosecond())
        .map_err(|_| TimingError::OutOfRange)
}

#[cfg(test)]
#[path = "croner_seam_test.rs"]
mod tests;
