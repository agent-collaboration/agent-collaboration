//! The time rule of B3's switch: exact milliseconds or a refusal.
use super::exact_millis;

#[test]
fn rfc3339_times_convert_to_exact_milliseconds_with_any_offset() {
    assert_eq!(
        exact_millis("2026-11-08T12:00:00.123Z", "t").ok(),
        Some(1_794_139_200_123)
    );
    assert_eq!(
        exact_millis("2026-11-08T13:00:00.123+01:00", "t").ok(),
        Some(1_794_139_200_123)
    );
    assert_eq!(
        exact_millis("2026-11-08T12:00:00Z", "t").ok(),
        Some(1_794_139_200_000)
    );
}

#[test]
fn a_finer_than_millisecond_or_unparseable_time_is_refused_not_rounded() {
    for invalid in [
        "2026-11-08T12:00:00.1234Z",
        "2026-11-08 12:00:00",
        "yesterday",
        "",
    ] {
        assert!(exact_millis(invalid, "t").is_err(), "{invalid}");
    }
}
