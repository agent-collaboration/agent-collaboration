//! Shared workerd helpers for the V6, V8, V9 and V12 event and alarm checks:
//! named wall times from the fixture's 10:00:00 mark, real alarm callbacks, and
//! the wake, notice-window and drain fixtures those checks need.
use super::super::{Statement, support::*};
use super::engine_probe_support::*;
use central_storage::{AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue};
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

pub(super) type Time = chrono::DateTime<chrono::Utc>;

/// The wall time `minutes:seconds.millis` after the fixture's 10:00:00 mark.
pub(super) fn at(start: Time, minutes: i64, seconds: i64, millis: i64) -> Time {
    start
        + chrono::Duration::minutes(minutes)
        + chrono::Duration::seconds(seconds)
        + chrono::Duration::milliseconds(millis)
}
pub(super) fn ms(time: Time) -> Option<i64> {
    Some(time.timestamp_millis())
}

/// Starts one due batch at `time` through the real alarm callback.
pub(super) async fn due_batch_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    set_now(fixture, time);
    runtime.after_event().await.unwrap();
}
pub(super) async fn callbacks(runtime: &Runtime) -> u64 {
    runtime.control("raw-alarm-count", &()).await.unwrap()
}

pub(super) async fn set_wake(fixture: &Fixture, number: u32, due: Time) -> workspace_ids::WakeId {
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "engine event".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    wake_id
}

/// A real central mail request that is answered in its own event.
pub(super) async fn send_message(
    fixture: &Fixture,
    number: u32,
) -> Result<CentralMailResult, central_storage::StoreFailure> {
    fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: CentralMailOperation::SendMessage {
                    to: session(&fixture.caller),
                    body: "traffic".to_owned().try_into().unwrap(),
                    delivery: workspace_model::central_mail::MessageDelivery::Auto,
                    reply_to: None,
                },
            },
        )
        .await
}

pub(super) async fn count(runtime: &Runtime, sql: &'static str, args: Vec<SqlValue>) -> i64 {
    match runtime
        .statement_host()
        .read(Statement { sql, args })
        .await
        .unwrap()
        .rows[0][0]
    {
        SqlValue::Integer(count) => count.get(),
        ref other => panic!("count: {other:?}"),
    }
}
pub(super) async fn notices(runtime: &Runtime) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM messages WHERE message_kind='notice'",
        vec![],
    )
    .await
}
pub(super) async fn wake_state(runtime: &Runtime, wake: &workspace_ids::WakeId) -> SqlValue {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state FROM wakes WHERE wake_id=?",
            args: vec![text(wake)],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone()
}

/// A second session joins a thread and the fixture caller posts in it at the
/// current time, so each recipient's notice window is due at the next
/// 15-minute slot after the post.
pub(super) async fn open_notice_window(fixture: &Fixture, first_request: u32) {
    let area = fixture.area(first_request).await;
    let thread = fixture.thread(first_request + 1, area).await;
    let mut observer = fixture.caller.clone();
    let mut address = session(&observer);
    address.session_id = "event-observer".to_owned().try_into().unwrap();
    observer.caller = m::Caller::Session(address);
    let joined = call(
        &fixture.host,
        &observer,
        Some(fixture.workspace.clone()),
        first_request + 2,
        m::WorkspaceOperation::JoinThread {
            thread_id: thread.clone(),
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(matches!(
        joined.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    created(
        fixture
            .call(
                first_request + 3,
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread,
                    body: "Notice activity".to_owned().try_into().unwrap(),
                    addressees: vec![],
                },
            )
            .await,
    );
}
pub(super) async fn window_due(runtime: &Runtime) -> Vec<Vec<SqlValue>> {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT DISTINCT due_at FROM notification_windows",
            args: vec![],
        })
        .await
        .unwrap()
        .rows
}
pub(super) fn stamp(time: Time) -> SqlValue {
    text(time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Presets the fixture workspace's drain backstop, as a held write would leave it.
pub(super) async fn preset_drain(fixture: &Fixture, runtime: &Runtime, due: Time) {
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(central_storage::StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=excluded.drain_due_at",
                        args: vec![
                            text(&fixture.workspace),
                            stamp(due),
                            stamp(due),
                            stamp(due)
                        ],
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}
pub(super) async fn drain_due(fixture: &Fixture, runtime: &Runtime) -> SqlValue {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone()
}
