//! Portable physical alarm selection, with the clock supplied by the host.
const MAX_ALARM_UNIX_MILLIS: i64 = i64::MAX / 1_000_000;
const FIVE_MINUTE_MILLIS: i64 = 300_000;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, thiserror::Error,
)]
#[serde(rename_all = "camelCase")]
pub enum AlarmPublicationFailure {
    #[error("this store has no engine publication clock")]
    ClockUnavailable,
    #[error("the decided time is outside the portable alarm range")]
    EarliestDueOutOfRange,
    #[error("the publication clock is outside the portable alarm range")]
    NowOutOfRange,
    #[error("no later five-minute mark is within the portable alarm range")]
    NoLaterGridMark,
}

/// Future times stay exact; a missed time moves to the next strict mark.
pub fn alarm_publication_time(
    earliest_due_unix_millis: Option<i64>,
    now_unix_millis: i64,
) -> Result<Option<i64>, AlarmPublicationFailure> {
    let Some(earliest_due_unix_millis) = earliest_due_unix_millis else {
        return Ok(None);
    };
    let supported = |value| (-MAX_ALARM_UNIX_MILLIS..=MAX_ALARM_UNIX_MILLIS).contains(&value);
    if !supported(earliest_due_unix_millis) {
        return Err(AlarmPublicationFailure::EarliestDueOutOfRange);
    }
    if !supported(now_unix_millis) {
        return Err(AlarmPublicationFailure::NowOutOfRange);
    }
    if earliest_due_unix_millis > now_unix_millis {
        return Ok(Some(earliest_due_unix_millis));
    }
    now_unix_millis
        .div_euclid(FIVE_MINUTE_MILLIS)
        .checked_add(1)
        .and_then(|mark| mark.checked_mul(FIVE_MINUTE_MILLIS))
        .filter(|mark| supported(*mark))
        .map(Some)
        .ok_or(AlarmPublicationFailure::NoLaterGridMark)
}

#[cfg(test)]
#[path = "alarm_publication_test.rs"]
mod tests;
