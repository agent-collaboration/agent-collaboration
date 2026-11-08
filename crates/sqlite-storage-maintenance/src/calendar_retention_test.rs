use super::{first_deletable_at, months_before};
use chrono::{DateTime, TimeDelta, Utc};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn a_mid_month_row_goes_one_millisecond_after_exactly_two_months() {
    let stamped = at("2026-01-15T10:00:00.000Z");

    let due = first_deletable_at(stamped, 2).unwrap();

    assert_eq!(due, at("2026-03-15T10:00:00.001Z"));
    assert!(months_before(due, 2).unwrap() > stamped);
    assert_eq!(
        months_before(due - TimeDelta::milliseconds(1), 2),
        Some(stamped)
    );
}

#[test]
fn a_row_on_the_last_day_of_a_long_month_waits_for_the_month_after_a_short_one() {
    // 2025-12-31 plus two months clamps to 2026-02-28, whose cutoff is 2025-12-28.
    let stamped = at("2025-12-31T12:00:00.000Z");

    let due = first_deletable_at(stamped, 2).unwrap();

    assert_eq!(due, at("2026-03-01T00:00:00.000Z"));
    assert_eq!(
        months_before(at("2026-02-28T23:59:59.999Z"), 2),
        Some(at("2025-12-28T23:59:59.999Z")),
        "the cutoff stands still at the short month's end"
    );
    assert!(months_before(due, 2).unwrap() > stamped);
}

#[test]
fn a_leap_day_cutoff_moves_on_with_the_leap_february() {
    let stamped = at("2027-12-30T08:00:00.000Z");

    let due = first_deletable_at(stamped, 2).unwrap();

    // 2028 is a leap year: 2028-02-29T08:00 still maps back to 2027-12-29.
    assert_eq!(due, at("2028-03-01T00:00:00.000Z"));
    assert!(months_before(due - TimeDelta::milliseconds(1), 2).unwrap() <= stamped);
}
