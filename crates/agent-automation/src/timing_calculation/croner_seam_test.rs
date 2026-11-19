//! The seam carries an instant across both jiff lines unchanged, on the DST edges.
use super::{TimingError, from_croner, next_occurrence, to_croner};
use chrono::{DateTime, Utc};

#[cfg(test)]
fn instant(text: &str) -> jiff::Timestamp {
    text.parse().unwrap()
}

#[cfg(test)]
fn utc_instant(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

#[test]
fn an_instant_crosses_the_seam_both_ways_unchanged_on_each_dst_edge() {
    // The last nanosecond before Toronto's 2026 spring gap, the gap's end, the
    // first 01:30 of the fall overlap and the repeated one.
    for text in [
        "2026-03-08T06:59:59.999999999Z",
        "2026-03-08T07:00:00Z",
        "2026-11-01T05:30:00Z",
        "2026-11-01T06:30:00.000000001Z",
    ] {
        let at = instant(text);

        let round_trip = from_croner(to_croner(at).unwrap()).unwrap();

        assert_eq!(round_trip, at, "{text} changed crossing the seam");
    }
}

#[test]
fn a_cron_across_the_seam_answers_on_both_dst_edges() {
    // Spring: 02:30 does not exist on March 8; it fires as the gap ends, 03:00 EDT.
    let spring = next_occurrence(
        "30 2 * * *",
        utc_instant("2026-03-08T05:00:00Z"),
        "America/Toronto",
    );
    // Fall: from inside the first 01:30, the repeated one is not a new day's run.
    let fall = next_occurrence(
        "30 1 * * *",
        utc_instant("2026-11-01T05:30:00Z"),
        "America/Toronto",
    );

    assert_eq!(spring.unwrap(), utc_instant("2026-03-08T07:00:00Z"));
    assert_eq!(fall.unwrap(), utc_instant("2026-11-02T06:30:00Z"));
}

#[test]
fn an_unknown_zone_name_is_refused_at_the_seam() {
    let refused = next_occurrence(
        "0 9 * * *",
        utc_instant("2026-03-08T00:00:00Z"),
        "Etc/Unknown",
    );

    assert!(matches!(refused, Err(TimingError::InvalidTimezone)));
}
