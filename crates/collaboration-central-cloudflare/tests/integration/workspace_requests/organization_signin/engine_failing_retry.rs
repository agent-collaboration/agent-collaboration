//! R4 on the workerd host: a failing subject is retried on its next try, never
//! earlier, whether or not its row decodes, even when a due batch lists it
//! through the 50 s look-ahead; and its retry always keeps a future alarm.
use super::engine_admin_support::*;

/// A wake whose target no longer decodes, failing once at its due mark.
async fn corrupt_wake_failing_once(engine: &EngineOrg) -> (String, Time) {
    let due = next_mark(engine.now());
    let wake = engine.wake(due, "broken target").await;
    engine
        .raw(
            "UPDATE automation_wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&wake)],
        )
        .await;
    engine.set(due);
    engine.after_event().await;
    let next_try = next_mark(due);
    assert_failing(engine, &wake, next_try).await;
    (wake, next_try)
}
async fn assert_failing(engine: &EngineOrg, wake: &str, next_try: Time) {
    assert_eq!(
        engine.record("wake_timer", wake).await[0][..3],
        [text("failing"), integer(1), stamp(next_try)]
    );
    let alarm = engine.alarm_ms().await;
    assert!(
        alarm.is_some_and(|alarm| alarm > engine.now().timestamp_millis()),
        "the retry at {next_try} has no future alarm: {alarm:?}"
    );
}

#[tokio::test]
async fn a_corrupt_failing_wake_waits_for_its_next_try_before_its_second_failure() {
    let engine = EngineOrg::new().await;
    let (wake, next_try) = corrupt_wake_failing_once(&engine).await;

    // A batch 20 s before the next try lists the wake through the look-ahead.
    engine.set(next_try - chrono::Duration::seconds(20));
    engine.after_event().await;
    assert_failing(&engine, &wake, next_try).await;

    // After the next try (and the 30 s floor) it fails again and retires.
    engine.set(next_try + chrono::Duration::seconds(30));
    engine.after_event().await;
    assert_eq!(
        engine.record("wake_timer", &wake).await[0][..3],
        [text("retired"), integer(2), central_storage::SqlValue::Null]
    );
    engine.close().await;
}

/// The stateful test's shrunk sequence: create, corrupt, advance 13 minutes,
/// restart. A crashed object starts with an empty in-memory floor, so a second
/// batch runs in the same 30 s, inside the look-ahead before the next try.
#[tokio::test]
async fn a_restart_inside_the_look_ahead_does_not_retire_a_failing_wake_early() {
    let engine = EngineOrg::new().await;
    let due = next_mark(engine.now());
    let wake = engine.wake(due, "broken target").await;
    engine
        .raw(
            "UPDATE automation_wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&wake)],
        )
        .await;
    engine.set(engine.now() + minutes(13));
    engine.after_event().await;
    let next_try = next_mark(engine.now());
    assert_failing(&engine, &wake, next_try).await;

    let crashed: Result<bool, _> = engine.runtime().await.control("crash", &()).await;
    assert!(crashed.is_err(), "the object restarts");
    engine.after_event().await;

    assert_failing(&engine, &wake, next_try).await;
    engine.close().await;
}
