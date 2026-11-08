//! Calendar-month retention: the cutoff a delete rule uses, and when that cutoff first
//! passes a stored row. Every store's rule and due time use this one arithmetic.
use chrono::{DateTime, Months, TimeDelta, Utc};

/// The cutoff a rule keeping `months` calendar months uses at `now`: a row stamped strictly
/// before it may be deleted. A day past the end of the earlier month clamps to its last day.
#[must_use]
pub fn months_before(now: DateTime<Utc>, months: u32) -> Option<DateTime<Utc>> {
    now.checked_sub_months(Months::new(months))
}

/// The first moment the rule allows deleting a row stamped `stamped`: the earliest `now`
/// whose cutoff is strictly after the stamp. Clamping makes the cutoff stand still at month
/// ends, so the first such moment can fall a few days after `stamped` plus `months`.
#[must_use]
pub fn first_deletable_at(stamped: DateTime<Utc>, months: u32) -> Option<DateTime<Utc>> {
    let deletable = |moment: DateTime<Utc>| {
        months_before(moment, months).is_some_and(|cutoff| cutoff > stamped)
    };
    // Adding then subtracting months never lands after the stamp, so `start` is not yet
    // deletable; a month-end clamp holds the cutoff back by at most three days.
    // Stores stamp whole milliseconds, so the search does too.
    let start = stamped.checked_add_months(Months::new(months))?;
    let mut not_yet = start.timestamp_millis();
    let mut already = start
        .checked_add_signed(TimeDelta::days(4))?
        .timestamp_millis();
    let moment = DateTime::<Utc>::from_timestamp_millis;
    if !moment(already).is_some_and(deletable) {
        return None;
    }
    while already - not_yet > 1 {
        let middle = not_yet + (already - not_yet) / 2;
        if moment(middle).is_some_and(deletable) {
            already = middle;
        } else {
            not_yet = middle;
        }
    }
    moment(already)
}

#[cfg(test)]
#[path = "calendar_retention_test.rs"]
mod tests;
