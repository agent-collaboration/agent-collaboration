use super::*;

#[tokio::test]
async fn steps_wake_sleepers_and_backwards_time_is_rejected_without_changing_now() {
    let initial: DateTime<Utc> = "2026-11-04T00:00:00Z".parse().unwrap();
    let clock = ManualClock::new(initial);
    let monotonic = clock.monotonic_now();
    let sleeper = clock.sleep_until(monotonic + std::time::Duration::from_secs(86400));
    tokio::pin!(sleeper);
    assert!(futures_util::poll!(&mut sleeper).is_pending());
    advance_clock_to(&clock, initial + chrono::Duration::days(1)).unwrap();
    assert!(futures_util::poll!(&mut sleeper).is_ready());
    assert_eq!(clock.utc_now(), initial + chrono::Duration::days(1));
    assert_eq!(
        clock.monotonic_now(),
        monotonic + std::time::Duration::from_secs(86400)
    );
    assert!(advance_clock_to(&clock, initial).is_err());
    assert_eq!(clock.utc_now(), initial + chrono::Duration::days(1));
}
