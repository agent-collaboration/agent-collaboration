use super::{first_deletable_at, months_before};
use chrono::{DateTime, TimeDelta, Utc};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn deletable(stamped: DateTime<Utc>, months: u32, moment: DateTime<Utc>) -> bool {
    months_before(moment, months).is_some_and(|cutoff| cutoff > stamped)
}

#[test]
fn a_mid_month_row_goes_one_millisecond_after_exactly_two_months() {
    let stamped = at("2026-01-15T10:00:00.000Z");

    let due = first_deletable_at(stamped, 2, stamped).unwrap();

    assert_eq!(due, at("2026-03-15T10:00:00.001Z"));
}

#[test]
fn a_row_on_the_last_day_of_a_long_month_waits_for_the_month_after_a_short_one() {
    // 2025-12-31 plus two months clamps to 2026-02-28, whose cutoff is 2025-12-28.
    let stamped = at("2025-12-31T12:00:00.000Z");

    let due = first_deletable_at(stamped, 2, stamped).unwrap();

    assert_eq!(due, at("2026-03-01T00:00:00.000Z"));
}

#[test]
fn a_leap_day_cutoff_moves_on_with_the_leap_february() {
    let stamped = at("2027-12-30T08:00:00.000Z");

    let due = first_deletable_at(stamped, 2, stamped).unwrap();

    // 2028-02-29T08:00 still maps back to 2027-12-29.
    assert_eq!(due, at("2028-03-01T00:00:00.000Z"));
}

#[test]
fn a_row_from_the_end_of_a_short_month_goes_at_its_first_crossing_not_after_the_clamp() {
    // From 2026-04-29 the cutoff clamps back to 2026-02-28 at the same time of day.
    let stamped = at("2026-02-28T12:00:00.000Z");

    assert_eq!(
        first_deletable_at(stamped, 2, stamped),
        Some(at("2026-04-28T12:00:00.001Z"))
    );
    assert_eq!(
        first_deletable_at(stamped, 2, at("2026-04-29T00:00:00.000Z")),
        Some(at("2026-04-29T12:00:00.001Z")),
        "inside the clamp, the cutoff passes the stamp again that afternoon"
    );
    assert_eq!(
        first_deletable_at(stamped, 2, at("2026-04-30T18:00:00.000Z")),
        Some(at("2026-04-30T18:00:00.000Z")),
        "already deletable at not_before"
    );
}

/// The first scanned moment at or after `from` when the real rule allows deletion. Each
/// minute is sampled at its start and one millisecond later, where a window opens when
/// the cutoff passes a minute-aligned stamp late in a day that clamps at midnight.
fn first_deletable_minute(
    stamped: DateTime<Utc>,
    months: u32,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let minute = TimeDelta::minutes(1);
    let mut moment = from;
    let millisecond = TimeDelta::milliseconds(1);
    while moment <= until {
        for sample in [moment, moment + millisecond] {
            if deletable(stamped, months, sample) {
                return Some(sample);
            }
        }
        moment += minute;
    }
    None
}

/// The helper's answer agrees with a minute-by-minute scan of the real rule: it is
/// deletable, the millisecond before it is not (unless it is `not_before` itself), and no
/// scanned minute before it was deletable.
fn assert_agrees_with_the_rule(stamped: DateTime<Utc>, months: u32, not_before: DateTime<Utc>) {
    let horizon = stamped + TimeDelta::days(i64::from(months) * 31 + 40);
    let due = first_deletable_at(stamped, months, not_before)
        .unwrap_or_else(|| panic!("no due time for {stamped} + {months} from {not_before}"));
    // Each month subtracted moves the cutoff back at least 28 days, so no moment earlier than
    // that can be deletable; the scan starts there.
    let earliest_possible = stamped + TimeDelta::days(i64::from(months) * 28);
    assert!(!deletable(stamped, months, earliest_possible));
    let scanned =
        first_deletable_minute(stamped, months, not_before.max(earliest_possible), horizon)
            .unwrap_or_else(|| panic!("the rule never allowed {stamped} + {months}"));
    let context =
        format!("stamp {stamped}, {months} months, from {not_before}: due {due}, scan {scanned}");
    assert!(due >= not_before, "{context}");
    assert!(
        deletable(stamped, months, due),
        "not deletable at its due time: {context}"
    );
    if due > not_before {
        assert!(
            !deletable(stamped, months, due - TimeDelta::milliseconds(1)),
            "deletable a millisecond earlier: {context}"
        );
    }
    assert!(
        due <= scanned && due > scanned - TimeDelta::minutes(1),
        "a deletable minute comes before the due time: {context}"
    );
}

#[test]
fn every_due_time_is_the_first_moment_the_real_rule_allows() {
    let days = [
        "2025-12-31",
        "2026-01-28",
        "2026-01-29",
        "2026-01-30",
        "2026-01-31",
        "2026-02-27",
        "2026-02-28",
        "2026-03-15",
        "2026-03-31",
        "2026-04-30",
        "2026-05-31",
        "2026-08-31",
        "2026-10-31",
        "2027-12-29",
        "2027-12-31",
        "2028-02-28",
        "2028-02-29",
        "2028-01-31",
    ];
    let times = [
        "00:00:00.000",
        "12:00:00.000",
        "23:59:00.000",
        "23:59:59.999",
    ];
    for months in [1, 2, 3, 6, 12] {
        for day in days {
            for time in times {
                let stamped = at(&format!("{day}T{time}Z"));
                let shifted = stamped + TimeDelta::days(i64::from(months) * 28);
                assert_agrees_with_the_rule(stamped, months, stamped);
                // Starting inside the window where a clamp can hold the cutoff back.
                for offset_hours in [0, 30, 54, 78, 102] {
                    assert_agrees_with_the_rule(
                        stamped,
                        months,
                        shifted + TimeDelta::hours(offset_hours),
                    );
                }
            }
        }
    }
}
