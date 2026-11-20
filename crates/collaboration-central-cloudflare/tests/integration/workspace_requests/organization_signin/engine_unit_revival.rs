//! V2 notice units and engine_source on the workerd host, through the real admin
//! route: a whole unit fails, recovers by itself, restarts its 60-day period,
//! retires at 60 days, stays stopped, and comes back only by EngineRetry.
use super::super::engine_evidence::{
    assert_every_written_attempt_id_is_uuid_v7, install_attempt_id_witness,
};
use super::engine_admin_support::*;
use central_storage::SqlValue;
use fleet_runtime::EngineReadFault;
use serde_json::json;
use workspace_model as m;

const WINDOWS: &str = "legacy_notice_windows";
const DRAINS: &str = "legacy_workspace_drains";

fn days(count: i64) -> chrono::Duration {
    chrono::Duration::days(count)
}

async fn notices(engine: &EngineOrg, recipient: &m::ActorIdentityId) -> i64 {
    engine
        .count(
            "SELECT COUNT(*) FROM mail_messages WHERE message_kind='notice' AND recipient_actor=?",
            vec![text(recipient)],
        )
        .await
}
async fn automation_messages(engine: &EngineOrg, automation: &str) -> i64 {
    engine
        .count(
            "SELECT COUNT(*) FROM mail_messages WHERE message_kind='automation' AND json_extract(body,'$.automationId')=?",
            vec![text(automation)],
        )
        .await
}
/// The recipient's open window and its due time.
async fn open_window(engine: &EngineOrg, recipient: &m::ActorIdentityId) -> (String, Time) {
    let rows = engine
        .rows(
            "SELECT window_id,due_at FROM mail_notification_windows WHERE recipient_actor=?",
            vec![text(recipient)],
        )
        .await;
    let [SqlValue::Text(window), SqlValue::Text(due)] = rows[0].as_slice() else {
        panic!("window shape: {rows:?}")
    };
    (window.clone(), due.parse().unwrap())
}
/// Corrupts one window by removing its items; returns them for the repair.
async fn break_window(engine: &EngineOrg, window: &str) -> Vec<Vec<SqlValue>> {
    let items = engine
        .rows(
            "SELECT window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at FROM mail_notification_window_items WHERE window_id=?",
            vec![text(window)],
        )
        .await;
    assert!(!items.is_empty());
    engine
        .raw(
            "DELETE FROM mail_notification_window_items WHERE window_id=?",
            vec![text(window)],
        )
        .await;
    items
}
async fn repair_window(engine: &EngineOrg, items: Vec<Vec<SqlValue>>) {
    for row in items {
        engine.raw("INSERT INTO mail_notification_window_items(window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?)", row).await;
    }
}
async fn unit_record(engine: &EngineOrg, kind: &str) -> Vec<Vec<SqlValue>> {
    engine.record(kind, "all").await
}
fn failing(count: i64, next: Time) -> Vec<Vec<SqlValue>> {
    vec![vec![
        text("failing"),
        integer(count),
        stamp(next),
        text("corrupt_row"),
    ]]
}

#[tokio::test]
async fn the_window_unit_recovers_restarts_its_period_retires_at_60_days_and_returns_by_retry() {
    let engine = EngineOrg::new().await;
    install_attempt_id_witness(engine.runtime().await.statement_host()).await;
    let thread = engine.thread().await;
    let recipient = engine.observer(&thread).await;
    engine.post(&thread).await;
    let (window, due) = open_window(&engine, &recipient).await;
    let wake = engine.wake(due, "healthy wake").await;
    let automation = engine.automation(due, "healthy automation").await;
    let items = break_window(&engine, &window).await;
    let base = engine.invocations().await.windows;

    engine.set(due);
    engine.after_event().await;

    assert_eq!(engine.wake_messages(&wake).await, 1);
    assert_eq!(automation_messages(&engine, &automation).await, 1);
    assert_eq!(
        unit_record(&engine, WINDOWS).await,
        failing(1, due + minutes(5))
    );
    assert_eq!(notices(&engine, &recipient).await, 0, "no window delivered");
    for count in 2..=3 {
        engine.set(due + minutes(5 * (count - 1)));
        engine.after_event().await;
        assert_eq!(
            unit_record(&engine, WINDOWS).await,
            failing(count, due + minutes(5 * count))
        );
    }
    assert_eq!(engine.invocations().await.windows, base + 3);

    // Automatic recovery before retirement.
    repair_window(&engine, items).await;
    engine.set(due + minutes(15));
    engine.after_event().await;
    assert!(
        unit_record(&engine, WINDOWS).await.is_empty(),
        "record deleted"
    );
    assert_eq!(
        notices(&engine, &recipient).await,
        1,
        "the Notice goes out once"
    );

    // Broken again: a new record at count 1 restarts the 60-day period.
    engine.post(&thread).await;
    let (window, first_failure) = open_window(&engine, &recipient).await;
    let items = break_window(&engine, &window).await;
    engine.set(first_failure);
    engine.after_event().await;
    assert_eq!(
        unit_record(&engine, WINDOWS).await,
        failing(1, first_failure + minutes(5))
    );
    let before_sixty = first_failure + days(60) - minutes(5);
    engine.set(before_sixty);
    engine.after_event().await;
    assert_eq!(
        unit_record(&engine, WINDOWS).await,
        failing(2, first_failure + days(60)),
        "one mark before 60 days: still failing"
    );
    engine.set(first_failure + days(60));
    engine.after_event().await;
    let retired = unit_record(&engine, WINDOWS).await;
    assert_eq!(
        retired[0][..3],
        [text("retired"), integer(3), SqlValue::Null],
        "at 60 days: retired"
    );
    let attempt = engine.attempt_id(WINDOWS, "all").await;
    assert_eq!(
        engine.discard(&attempt).await,
        refused("discardUnsupported")
    );
    let health = engine.health().await;
    assert_eq!(kind_health(&health, WINDOWS)["retired"], 1);

    // Retired stays retired, even repaired.
    repair_window(&engine, items).await;
    let stopped = engine.invocations().await.windows;
    let mut last = first_failure + days(60);
    for step in 1..=4 {
        last = first_failure + days(60) + minutes(5 * step);
        engine.set(last);
        engine.after_event().await;
    }
    assert_eq!(
        engine.invocations().await.windows,
        stopped,
        "invoked zero times"
    );
    assert_eq!(notices(&engine, &recipient).await, 1, "no Notice");

    // Admin revival.
    assert_eq!(
        engine.retry(&attempt).await,
        json!({"result":"engineRetried"})
    );
    assert!(unit_record(&engine, WINDOWS).await.is_empty());
    let alarm = engine.alarm_ms().await.expect("the alarm is published");
    assert_eq!(
        alarm,
        (last + chrono::Duration::seconds(30)).timestamp_millis(),
        "P4 floor after the last batch"
    );
    assert_eq!(engine.invocations().await.windows, stopped);
    engine.set(chrono::DateTime::from_timestamp_millis(alarm).unwrap());
    engine.after_event().await;
    assert_eq!(engine.invocations().await.windows, stopped + 1, "runs once");
    assert_eq!(
        notices(&engine, &recipient).await,
        2,
        "the Notice goes out once"
    );
    assert!(
        unit_record(&engine, WINDOWS).await.is_empty(),
        "no record remains"
    );
    assert_every_written_attempt_id_is_uuid_v7(
        engine.runtime().await.statement_host(),
        "the_window_unit_recovers_restarts_its_period_retires_at_60_days_and_returns_by_retry",
        &[],
    )
    .await;
    engine.close().await;
}

/// A committed post the drain has not taken yet: a seeded future gate skips the
/// request's own drain, then the seed goes, leaving the +5 s backstop.
async fn undrained_post(engine: &EngineOrg, thread: &m::ThreadId) {
    let now = engine.now();
    let seed = engine.fresh();
    engine.raw("INSERT INTO engine_work_attempts VALUES (?,'legacy_workspace_drains','all','failing',1,?,'statement_rejected',NULL,?,?,NULL,?,?)",
        vec![text(&seed), stamp(now + minutes(60)), stamp(now), stamp(now), stamp(now), stamp(now)]).await;
    engine.post(thread).await;
    engine
        .raw(
            "DELETE FROM engine_work_attempts WHERE attempt_id=?",
            vec![text(&seed)],
        )
        .await;
}
async fn drain_row(engine: &EngineOrg) -> Vec<SqlValue> {
    engine
        .rows(
            "SELECT drained_through_position,drain_due_at FROM mail_workspace_drains WHERE workspace_id=?",
            vec![text(&engine.workspace)],
        )
        .await
        .remove(0)
}
async fn break_drain(engine: &EngineOrg) -> SqlValue {
    let row = drain_row(engine).await;
    assert!(
        matches!(row[1], SqlValue::Text(_)),
        "the backstop is set: {row:?}"
    );
    engine
        .raw(
            "UPDATE mail_workspace_drains SET drained_through_position='bad' WHERE workspace_id=?",
            vec![text(&engine.workspace)],
        )
        .await;
    row[0].clone()
}
async fn repair_drain(engine: &EngineOrg, cursor: SqlValue) {
    engine
        .raw(
            "UPDATE mail_workspace_drains SET drained_through_position=? WHERE workspace_id=?",
            vec![cursor, text(&engine.workspace)],
        )
        .await;
}
async fn window_items(engine: &EngineOrg, thread: &m::ThreadId) -> i64 {
    engine
        .count(
            "SELECT COUNT(*) FROM mail_notification_window_items WHERE thread_id=?",
            vec![text(thread)],
        )
        .await
}
fn position(value: &SqlValue) -> i64 {
    match value {
        SqlValue::Integer(position) => position.get(),
        other => panic!("cursor: {other:?}"),
    }
}
fn corrupt_drain(count: i64, next: Time) -> Vec<Vec<SqlValue>> {
    failing(count, next)
}

#[tokio::test]
async fn the_drain_unit_recovers_restarts_its_period_retires_at_60_days_and_returns_by_retry() {
    let engine = EngineOrg::new().await;
    install_attempt_id_witness(engine.runtime().await.statement_host()).await;
    let thread = engine.thread().await;
    engine.observer(&thread).await;
    undrained_post(&engine, &thread).await;
    let cursor = break_drain(&engine).await;
    let due = next_mark(engine.now());
    let wake = engine.wake(due, "healthy wake").await;
    let automation = engine.automation(due, "healthy automation").await;
    let items_before = window_items(&engine, &thread).await;
    let base = engine.invocations().await;

    engine.set(due);
    engine.after_event().await;

    assert_eq!(engine.wake_messages(&wake).await, 1);
    assert_eq!(automation_messages(&engine, &automation).await, 1);
    assert_eq!(
        unit_record(&engine, DRAINS).await,
        corrupt_drain(1, due + minutes(5))
    );
    assert_eq!(
        window_items(&engine, &thread).await,
        items_before,
        "nothing drained"
    );
    for count in 2..=3 {
        engine.set(due + minutes(5 * (count - 1)));
        engine.after_event().await;
        assert_eq!(
            unit_record(&engine, DRAINS).await,
            corrupt_drain(count, due + minutes(5 * count))
        );
    }
    assert_eq!(engine.invocations().await.drains, base.drains + 3);

    // Automatic recovery: the drain runs once and the record goes.
    repair_drain(&engine, cursor.clone()).await;
    engine.set(due + minutes(15));
    engine.after_event().await;
    assert!(
        unit_record(&engine, DRAINS).await.is_empty(),
        "record deleted"
    );
    let drained = drain_row(&engine).await;
    assert_eq!(drained[1], SqlValue::Null, "drained");
    assert!(
        position(&drained[0]) > position(&cursor),
        "the cursor moves past the post: {drained:?} from {cursor:?}"
    );
    engine.set(due + minutes(20));
    engine.after_event().await;
    assert_eq!(drain_row(&engine).await, drained, "drained once, not again");

    // Broken again: a new record at count 1.
    undrained_post(&engine, &thread).await;
    let cursor = break_drain(&engine).await;
    let first_failure = next_mark(engine.now());
    engine.set(first_failure);
    engine.after_event().await;
    assert_eq!(
        unit_record(&engine, DRAINS).await,
        corrupt_drain(1, first_failure + minutes(5))
    );
    engine.set(first_failure + days(60) - minutes(5));
    engine.after_event().await;
    assert_eq!(
        unit_record(&engine, DRAINS).await,
        corrupt_drain(2, first_failure + days(60)),
        "one mark before 60 days: still failing"
    );
    engine.set(first_failure + days(60));
    engine.after_event().await;
    assert_eq!(
        unit_record(&engine, DRAINS).await[0][..3],
        [text("retired"), integer(3), SqlValue::Null]
    );
    let attempt = engine.attempt_id(DRAINS, "all").await;
    assert_eq!(
        engine.discard(&attempt).await,
        refused("discardUnsupported")
    );

    // Retired stays retired, even repaired.
    repair_drain(&engine, cursor).await;
    let stopped = engine.invocations().await;
    let pending = drain_row(&engine).await;
    let mut last = first_failure + days(60);
    for step in 1..=4 {
        last = first_failure + days(60) + minutes(5 * step);
        engine.set(last);
        engine.after_event().await;
    }
    assert_eq!(
        (
            engine.invocations().await.drains,
            engine.invocations().await.request_drains
        ),
        (stopped.drains, stopped.request_drains),
        "neither drain path runs"
    );
    assert_eq!(drain_row(&engine).await, pending, "nothing drained");

    // Admin revival.
    assert_eq!(
        engine.retry(&attempt).await,
        json!({"result":"engineRetried"})
    );
    assert!(unit_record(&engine, DRAINS).await.is_empty());
    let alarm = engine.alarm_ms().await.expect("the alarm is published");
    assert_eq!(
        alarm,
        (last + chrono::Duration::seconds(30)).timestamp_millis()
    );
    engine.set(chrono::DateTime::from_timestamp_millis(alarm).unwrap());
    engine.after_event().await;
    assert_eq!(
        engine.invocations().await.drains,
        stopped.drains + 1,
        "runs once"
    );
    assert_eq!(drain_row(&engine).await[1], SqlValue::Null, "drained once");
    assert!(
        unit_record(&engine, DRAINS).await.is_empty(),
        "no record remains"
    );
    assert_every_written_attempt_id_is_uuid_v7(
        engine.runtime().await.statement_host(),
        "the_drain_unit_recovers_restarts_its_period_retires_at_60_days_and_returns_by_retry",
        &[],
    )
    .await;
    engine.close().await;
}

#[tokio::test]
async fn a_retired_engine_source_is_never_read_until_retry_and_other_kinds_run() {
    let engine = EngineOrg::new().await;
    install_attempt_id_witness(engine.runtime().await.statement_host()).await;
    let first = next_mark(engine.now());
    let wake = engine.wake(first, "wake behind a broken source").await;
    let automation = engine.automation(first, "other kind").await;
    let runtime = engine.runtime().await;
    let fault = || EngineReadFault {
        kind: "wake_timer".into(),
    };
    runtime.inject_engine_read_fault(fault()).await;
    engine.set(first);
    engine.after_event().await;
    assert_eq!(
        engine.record("engine_source", "wake_timer").await,
        failing(1, first + minutes(5))
    );
    assert_eq!(
        automation_messages(&engine, &automation).await,
        1,
        "other kinds run"
    );
    assert_eq!(engine.wake_messages(&wake).await, 0);

    runtime.inject_engine_read_fault(fault()).await;
    engine.set(first + minutes(5));
    engine.after_event().await;
    assert_eq!(
        engine.record("engine_source", "wake_timer").await[0][..2],
        [text("retired"), integer(2)],
        "the second failure retires it"
    );

    // Arm one more read fault: it stays armed only if the kind is never read.
    runtime.inject_engine_read_fault(fault()).await;
    let later = engine
        .automation(first + minutes(15), "later other kind")
        .await;
    let mut last = first + minutes(5);
    for step in 2..=5 {
        last = first + minutes(5 * step);
        engine.set(last);
        engine.after_event().await;
    }
    assert_eq!(
        engine.wake_messages(&wake).await,
        0,
        "the kind is never read"
    );
    assert_eq!(
        automation_messages(&engine, &later).await,
        1,
        "other kinds run"
    );

    let health = engine.health().await;
    assert_eq!(kind_health(&health, "engine_source")["retired"], 1);
    let page = engine.retired(None, 100).await;
    let listed: Vec<_> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "engine_source")
        .collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["subjectKey"], "wake_timer");
    let attempt = engine.attempt_id("engine_source", "wake_timer").await;
    assert_eq!(listed[0]["attemptId"], attempt.as_str());
    assert_eq!(
        engine.discard(&attempt).await,
        refused("discardUnsupported")
    );

    assert_eq!(
        engine.retry(&attempt).await,
        json!({"result":"engineRetried"})
    );
    assert!(
        engine
            .record("engine_source", "wake_timer")
            .await
            .is_empty()
    );
    let alarm = engine.alarm_ms().await.expect("alarm published");
    assert_eq!(
        alarm,
        (last + chrono::Duration::seconds(30)).timestamp_millis()
    );
    engine.set(chrono::DateTime::from_timestamp_millis(alarm).unwrap());
    engine.after_event().await;
    assert_eq!(
        engine.record("engine_source", "wake_timer").await[0][..2],
        [text("failing"), integer(1)],
        "the next batch reads the kind: the fault armed while retired is consumed now"
    );
    let next = next_mark(engine.now());
    engine.set(next);
    engine.after_event().await;
    assert_eq!(engine.wake_messages(&wake).await, 1, "the kind runs again");
    assert!(
        engine
            .record("engine_source", "wake_timer")
            .await
            .is_empty()
    );
    assert_every_written_attempt_id_is_uuid_v7(
        engine.runtime().await.statement_host(),
        "a_retired_engine_source_is_never_read_until_retry_and_other_kinds_run",
        &[],
    )
    .await;
    engine.close().await;
}
