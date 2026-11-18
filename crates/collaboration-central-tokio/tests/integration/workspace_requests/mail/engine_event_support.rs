//! Shared native helpers for the V6, V9 and V12 event and alarm checks: named
//! wall times from the fixture's 10:00:00 mark, the held-clock event driver,
//! and the wake, notice-window and drain fixtures those checks need.
use super::super::{Statement, support::*};
use super::engine_probe_support::*;
use central_storage::SqlValue;
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
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
pub(super) fn sync(time: Time) -> SyncTime {
    SyncTime::from_unix_millis(time.timestamp_millis())
}

/// The native driver computed its deadline for the published alarm at the
/// current reading, so a later UTC move cannot make it fire on its own.
pub(super) async fn driver_settled(fixture: &Fixture, runtime: &Runtime) {
    if let Some(alarm) = alarm_ms(runtime) {
        fixture
            .clock
            .not_due_check_now(
                fixture
                    .clock
                    .wall_deadline(SyncTime::from_unix_millis(alarm)),
            )
            .await;
    }
}

/// Starts one due batch at `time` through the event path.
pub(super) async fn due_batch_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    driver_settled(fixture, runtime).await;
    fixture.set_utc(time).await;
    runtime.after_event().await.unwrap();
    driver_settled(fixture, runtime).await;
}

/// Lets the real alarm owner fire its published deadline at `time`.
pub(super) async fn driver_fires_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    assert_eq!(alarm_ms(runtime), ms(time), "the published alarm");
    driver_settled(fixture, runtime).await;
    let completed = runtime.alarm_completion_count();
    fixture.advance_to(sync(time)).await;
    runtime.until_alarm_completed(completed + 1).await;
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
/// current time, so the observer's notice window is due at the next
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
