use super::*;
use hegel::{TestCase, generators as gs};

/// 2030-01-01T00:00:00Z, on the 5-minute grid.
const BASE: i64 = 1_893_456_000_000;

fn at(millis: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(BASE + millis).unwrap()
}
const MINUTE: i64 = 60_000;

#[test]
fn the_cancel_lands_on_the_first_mark_at_least_17_minutes_after_sending() {
    let cases = [
        // (sent, accepted, expected cancel), in ms from BASE.
        (0, 0, 20 * MINUTE),                   // 17 min -> the 20-min mark
        (3 * MINUTE, 3 * MINUTE, 20 * MINUTE), // exactly 20 min: already on a mark
        (3 * MINUTE + 1, 3 * MINUTE + 1, 25 * MINUTE), // 1 ms past a mark rounds up
        (0, 4 * MINUTE, 20 * MINUTE),          // accepted later: counted from sending
        (0, 15 * MINUTE, 20 * MINUTE),         // accepted exactly 15 min after sending
        (0, 20 * MINUTE, 25 * MINUTE), // older than 15 min: clamped to accepted − 15 (5) -> 22 -> 25
        (10 * MINUTE, 0, 20 * MINUTE), // a fast Mac clock is clamped to accepted
    ];
    for (sent, accepted, expected) in cases {
        assert_eq!(
            cancel_due_at(at(sent), at(accepted)),
            at(expected),
            "sent {sent} accepted {accepted}"
        );
    }
}

#[test]
fn the_window_closes_exactly_15_minutes_after_the_clamped_send() {
    assert!(delivery_window_open(at(0), at(0), at(15 * MINUTE - 1)));
    assert!(!delivery_window_open(at(0), at(0), at(15 * MINUTE)));
    // A fast Mac clock cannot keep the window open longer.
    assert!(!delivery_window_open(
        at(10 * MINUTE),
        at(0),
        at(15 * MINUTE)
    ));
}

#[test]
fn system_expiry_uses_the_first_grid_mark_at_or_after_24_hours() {
    let day = 24 * 60 * MINUTE;
    assert_eq!(system_message_deadline(at(0)), Some(at(day)));
    assert_eq!(system_message_deadline(at(1)), Some(at(day + 5 * MINUTE)));
    assert_eq!(
        system_message_deadline(at(5 * MINUTE - 1)),
        Some(at(day + 5 * MINUTE))
    );
}

#[hegel::test]
fn the_cancel_is_on_the_grid_after_the_window_and_margin_and_within_one_step(tc: TestCase) {
    let accepted = tc.draw(
        gs::integers::<i64>()
            .min_value(0)
            .max_value(10_000 * MINUTE),
    );
    let skew = tc.draw(
        gs::integers::<i64>()
            .min_value(-60 * MINUTE)
            .max_value(60 * MINUTE),
    );
    let sent = accepted + skew;

    let due = cancel_due_at(at(sent), at(accepted));

    let clamped = sent.clamp(accepted - 15 * MINUTE, accepted);
    let due_ms = due.timestamp_millis() - BASE;
    assert_eq!(due_ms.rem_euclid(5 * MINUTE), 0, "on the 5-minute grid");
    assert!(
        due_ms >= clamped + 17 * MINUTE,
        "never before the window and margin"
    );
    assert!(
        due_ms < clamped + 22 * MINUTE,
        "within one grid step after them"
    );
    assert!(
        !delivery_window_open(at(sent), at(accepted), due),
        "the window is closed at the cancel"
    );
}
