//! Shared V2 helpers on the workerd host: the failure record, mark arithmetic,
//! the held clock and today's wake and automation requests.
use super::{Statement, support::*, workerd_host::RemoteRuntime};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use std::sync::{Arc, atomic::Ordering};
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as MailResult,
};

pub(super) type Runtime = Arc<RemoteRuntime>;
pub(super) type Time = chrono::DateTime<chrono::Utc>;

pub(super) fn minutes(count: i64) -> chrono::Duration {
    chrono::Duration::minutes(count)
}
pub(super) fn stamp(at: Time) -> SqlValue {
    text(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub(super) fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}
/// The first 5-minute mark strictly after `at`.
pub(super) fn next_mark(at: Time) -> Time {
    let millis = at.timestamp_millis();
    chrono::DateTime::from_timestamp_millis((millis.div_euclid(300_000) + 1) * 300_000).unwrap()
}
pub(super) fn now(fixture: &Fixture) -> Time {
    chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap()
}
pub(super) fn set_now(fixture: &Fixture, at: Time) {
    fixture
        .clock
        .0
        .store(at.timestamp_millis(), Ordering::SeqCst);
}
pub(super) async fn runtime(fixture: &Fixture) -> Runtime {
    fixture.host.central(&fixture.caller.organization_id).await
}
pub(super) async fn rows(
    runtime: &Runtime,
    sql: &'static str,
    args: Vec<SqlValue>,
) -> Vec<Vec<SqlValue>> {
    runtime.read(Statement { sql, args }).await.unwrap().rows
}
/// Runs one raw statement, for corruptions, repairs and test triggers only.
pub(super) async fn raw(runtime: &Runtime, sql: &'static str, args: Vec<SqlValue>) {
    assert!(matches!(
        runtime
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args },
                    expect: RowExpectation::Any,
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}
pub(super) async fn alarm_ms(runtime: &Runtime) -> Option<i64> {
    runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap()
}
/// (state, failure_count, next_try_at, last_failure, last_failure_detail)
pub(super) async fn record(runtime: &Runtime, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
    rows(
        runtime,
        "SELECT state,failure_count,next_try_at,last_failure,last_failure_detail FROM central_work_attempts WHERE machine_kind=? AND subject_key=?",
        vec![text(kind), text(key)],
    )
    .await
}
pub(super) async fn whole_record(runtime: &Runtime, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
    rows(
        runtime,
        "SELECT * FROM central_work_attempts WHERE machine_kind=? AND subject_key=?",
        vec![text(kind), text(key)],
    )
    .await
}
pub(super) async fn count(runtime: &Runtime, sql: &'static str, args: Vec<SqlValue>) -> i64 {
    match &rows(runtime, sql, args).await[0][0] {
        SqlValue::Integer(count) => count.get(),
        other => panic!("count: {other:?}"),
    }
}
pub(super) async fn wake_messages(runtime: &Runtime, wake: &str) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?",
        vec![text(wake)],
    )
    .await
}
pub(super) async fn automation_messages(runtime: &Runtime, automation: &str) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM messages WHERE message_kind='automation' AND json_extract(body,'$.automationId')=?",
        vec![text(automation)],
    )
    .await
}
pub(super) async fn all_messages(runtime: &Runtime) -> i64 {
    count(runtime, "SELECT COUNT(*) FROM messages", vec![]).await
}
pub(super) async fn set_wake(fixture: &Fixture, request: u32, due: Time, label: &str) -> String {
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(request),
                operation: Operation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: label.to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let MailResult::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    wake_id.to_string()
}
pub(super) async fn set_automation_at(
    fixture: &Fixture,
    request: u32,
    at: Time,
    label: &str,
) -> String {
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(request),
                operation: Operation::SetAutomation {
                    target: session(&fixture.caller),
                    timing: serde_json::from_value(serde_json::json!({"kind":"at","at":at}))
                        .unwrap(),
                    text: label.to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let MailResult::Done(Answer::AutomationSet { automation_id, .. }) = result else {
        panic!("automation refused: {result:?}")
    };
    automation_id.to_string()
}
