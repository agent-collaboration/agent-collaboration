//! Calendar-month retention: the cutoff a delete rule uses, and when that cutoff first
//! passes a stored row. Every store's rule and due time use this one arithmetic.
use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveTime, TimeDelta, Utc};

/// The cutoff a rule keeping `months` calendar months uses at `now`: a row stamped strictly
/// before it may be deleted. A day past the end of the earlier month clamps to its last day.
#[must_use]
pub fn months_before(now: DateTime<Utc>, months: u32) -> Option<DateTime<Utc>> {
    now.checked_sub_months(Months::new(months))
}

/// The first moment at or after `not_before` when the rule allows deleting a row stamped
/// `stamped`: the earliest such `now` whose [`months_before`] cutoff is strictly after the
/// stamp.
///
/// The cutoff is not monotone. In the trailing days of a month longer than the one
/// `months` earlier, it clamps back to that shorter month's last day at the same time of
/// day, so a row deletable one afternoon is not deletable again until the next afternoon
/// or the next month. Every candidate below is a moment where the cutoff can first pass
/// the stamp: the stamp shifted by `months` plus one millisecond, the stamp's time of day
/// plus one millisecond on the day `not_before` falls on or the next, and the first
/// moment of the month after.
#[must_use]
pub fn first_deletable_at(
    stamped: DateTime<Utc>,
    months: u32,
    not_before: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let deletable = |moment: &DateTime<Utc>| {
        months_before(*moment, months).is_some_and(|cutoff| cutoff > stamped)
    };
    let millisecond = TimeDelta::milliseconds(1);
    let crossing = stamped.checked_add_months(Months::new(months))? + millisecond;
    let start = not_before.max(crossing);
    if deletable(&start) {
        return Some(start);
    }
    let time_of_day = stamped.time();
    let on_day = |day: NaiveDate| -> Option<DateTime<Utc>> {
        Some(day.and_time(time_of_day).and_utc() + millisecond)
    };
    let day = start.date_naive();
    let next_month = first_moment_of_month_after(day)?;
    [
        on_day(day),
        day.succ_opt().and_then(on_day),
        Some(next_month),
    ]
    .into_iter()
    .flatten()
    .filter(|candidate| *candidate >= start && deletable(candidate))
    .min()
}

fn first_moment_of_month_after(day: NaiveDate) -> Option<DateTime<Utc>> {
    let first_of_month = day.with_day(1)?;
    Some(
        first_of_month
            .checked_add_months(Months::new(1))?
            .and_time(NaiveTime::MIN)
            .and_utc(),
    )
}

#[cfg(test)]
#[path = "calendar_retention_test.rs"]
mod tests;
