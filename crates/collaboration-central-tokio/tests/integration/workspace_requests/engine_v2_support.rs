//! Shared V2 helpers on the native host: the failure record, mark arithmetic,
//! today's wake and automation requests and the engine observation capture.
use super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use collaboration_central_core::WorkspaceRuntime;
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceStores};
use serde_json::Value;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as MailResult,
};

pub(super) type Runtime = Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>;
pub(super) type Time = chrono::DateTime<chrono::Utc>;

pub(super) const FIVE_MINUTES_MS: i64 = 300_000;

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
    chrono::DateTime::from_timestamp_millis(
        (millis.div_euclid(FIVE_MINUTES_MS) + 1) * FIVE_MINUTES_MS,
    )
    .unwrap()
}
pub(super) async fn runtime(fixture: &Fixture) -> Runtime {
    fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap()
}
pub(super) async fn rows(
    runtime: &Runtime,
    sql: &'static str,
    args: Vec<SqlValue>,
) -> Vec<Vec<SqlValue>> {
    runtime
        .runtime()
        .statement_host()
        .read(Statement { sql, args })
        .await
        .unwrap()
        .rows
}
/// Runs one raw statement, for corruptions, repairs and test triggers only.
pub(super) async fn raw(runtime: &Runtime, sql: &'static str, args: Vec<SqlValue>) {
    assert!(matches!(
        runtime
            .runtime()
            .statement_host()
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
pub(super) fn alarm_ms(runtime: &Runtime) -> Option<i64> {
    runtime
        .runtime()
        .statement_host()
        .alarm()
        .map(|time| time.unix_millis())
}
/// (state, failure_count, next_try_at, last_failure, last_failure_detail)
pub(super) async fn record(runtime: &Runtime, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
    rows(
        runtime,
        "SELECT state,failure_count,next_try_at,last_failure,last_failure_detail FROM engine_work_attempts WHERE machine_kind=? AND subject_key=?",
        vec![text(kind), text(key)],
    )
    .await
}
/// Every column of the record, to prove a retired one is never touched.
pub(super) async fn whole_record(runtime: &Runtime, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
    rows(
        runtime,
        "SELECT * FROM engine_work_attempts WHERE machine_kind=? AND subject_key=?",
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
        "SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?",
        vec![text(wake)],
    )
    .await
}
pub(super) async fn automation_messages(runtime: &Runtime, automation: &str) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM mail_messages WHERE message_kind='automation' AND json_extract(body,'$.automationId')=?",
        vec![text(automation)],
    )
    .await
}
pub(super) async fn all_messages(runtime: &Runtime) -> i64 {
    count(runtime, "SELECT COUNT(*) FROM mail_messages", vec![]).await
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

/// Captures the native sink's `EngineItemFailed` lines. The native host logs
/// each observation through `tracing` (native_logging.rs); a thread-local
/// subscriber on the current-thread test runtime sees every one.
#[derive(Clone, Default)]
pub(super) struct EngineObservations(Arc<Mutex<Vec<u8>>>);
impl Write for EngineObservations {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// One observed failure: (kind, key, class, count, retired, workspaces).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ItemFailed {
    pub kind: String,
    pub key: String,
    pub class: String,
    pub count: i64,
    pub retired: bool,
    pub workspaces: String,
}
impl EngineObservations {
    pub(super) fn install(&self) -> tracing::subscriber::DefaultGuard {
        let output = self.clone();
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .json()
                .with_ansi(false)
                .without_time()
                .with_writer(move || output.clone())
                .finish(),
        )
    }
    pub(super) fn failures(&self) -> Vec<ItemFailed> {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| {
                event.pointer("/fields/message").and_then(Value::as_str)
                    == Some("central engine item failed")
            })
            .map(|event| {
                let field = |name: &str| event.pointer(&format!("/fields/{name}")).cloned();
                ItemFailed {
                    kind: field("kind")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap(),
                    key: field("key")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap(),
                    class: field("class")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap(),
                    count: field("count").and_then(|v| v.as_i64()).unwrap(),
                    retired: field("retired").and_then(|v| v.as_bool()).unwrap(),
                    workspaces: field("workspaces")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                }
            })
            .collect()
    }
    pub(super) fn for_key(&self, key: &str) -> Vec<ItemFailed> {
        self.failures()
            .into_iter()
            .filter(|failure| failure.key == key)
            .collect()
    }
}
