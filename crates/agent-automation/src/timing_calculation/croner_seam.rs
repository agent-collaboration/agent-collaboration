//! The one place croner's jiff 0.2 types appear.
//!
//! croner's `jiff` feature is built on jiff 0.2, while our code uses jiff 1.x.
//! Only a Unix timestamp and an IANA zone name cross this seam, in each direction.
//! When croner ships a jiff-1 feature, this module and the `jiff-croner`
//! dependency go.
use super::TimingError;
use croner::Cron;

/// The first occurrence of `cron` strictly after `start`, read in the IANA zone `zone`.
pub(super) fn next_occurrence(
    cron: &Cron,
    start: jiff::Timestamp,
    zone: &str,
) -> Result<jiff::Timestamp, TimingError> {
    let zone = jiff_croner::tz::TimeZone::get(zone).map_err(|_| TimingError::InvalidTimezone)?;
    let start = to_croner(start)?.to_zoned(zone);
    let due = cron
        .find_next_occurrence(&start, false)
        .map_err(|_| TimingError::OutOfRange)?;
    from_croner(due.timestamp())
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
