use super::{aligned_at_or_after, on_five_minute_grid as on_grid};
use crate::EngineTime;
use chrono::{DateTime, Utc};

fn timestamp(value: &str) -> EngineTime {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn grid_alignment_includes_fractional_and_negative_timestamps() {
    let mark = timestamp("2026-11-14T12:05:00Z");
    assert!(on_grid(mark));
    assert_eq!(aligned_at_or_after(mark, 300), Some(mark));
    let fractional = timestamp("2026-11-14T12:05:00.000000001Z");
    assert!(!on_grid(fractional));
    assert_eq!(
        aligned_at_or_after(fractional, 300),
        Some(timestamp("2026-11-14T12:10:00Z"))
    );
    assert_eq!(
        aligned_at_or_after(timestamp("1969-12-31T23:59:59.5Z"), 300),
        Some(timestamp("1970-01-01T00:00:00Z"))
    );
}

#[test]
fn shared_cadence_slots_cross_hours_and_exclude_the_current_slot() {
    for (activity, cadence, expected) in [
        ("2030-01-01T00:01:00Z", 1500, "2030-01-01T00:25:00Z"),
        ("2030-01-01T00:14:00Z", 1500, "2030-01-01T00:25:00Z"),
        ("2030-01-01T00:26:00Z", 1500, "2030-01-01T00:50:00Z"),
        ("2030-01-01T00:40:00Z", 1500, "2030-01-01T00:50:00Z"),
        ("2030-01-01T00:51:00Z", 1500, "2030-01-01T01:15:00Z"),
        ("2030-01-01T01:09:00Z", 1500, "2030-01-01T01:15:00Z"),
        ("2030-01-01T00:25:00Z", 1500, "2030-01-01T00:50:00Z"),
        ("2030-01-01T00:01:00Z", 3300, "2030-01-01T00:25:00Z"),
        ("2030-01-01T00:25:00Z", 3300, "2030-01-01T01:20:00Z"),
        ("2030-01-01T00:25:00Z", 900, "2030-01-01T00:30:00Z"),
        ("2030-01-01T00:26:00Z", 900, "2030-01-01T00:30:00Z"),
        ("2030-01-01T00:26:00Z", 3300, "2030-01-01T01:20:00Z"),
        (
            "1969-12-31T23:59:59.999999999Z",
            1500,
            "1970-01-01T00:00:00Z",
        ),
        ("1970-01-01T00:00:00Z", 1500, "1970-01-01T00:25:00Z"),
    ] {
        assert_eq!(
            super::aligned_strictly_after(timestamp(activity), cadence),
            Some(timestamp(expected)),
            "{activity}, {cadence}"
        );
    }
}
