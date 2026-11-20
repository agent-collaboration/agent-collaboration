use super::super::{Statement, support::*};
use router_clock::{ClockInstant, RuntimeClock};
use std::time::Duration;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};

#[tokio::test]
async fn earlier_timer_waits_for_old_publication_and_becomes_the_driver_deadline() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let start = fixture.clock.utc_now();
    let earlier = start + chrono::Duration::seconds(300);
    let later = start + chrono::Duration::seconds(600);
    let make_request = |number, due_at| CentralMailRequest {
        request_id: id(number),
        operation: Operation::SetWake {
            target: session(&fixture.caller),
            due_at,
            text: "ordered".to_owned().try_into().unwrap(),
        },
    };
    let old_request = make_request(1400, later);
    let new_request = make_request(1401, earlier);
    let mut publication = runtime.runtime().statement_host().hold_alarm_publication();
    let old_runtime = runtime.clone();
    let old_caller = fixture.caller.clone();
    let old = tokio::spawn(async move {
        old_runtime
            .request_central(&old_caller, old_request)
            .await
            .unwrap()
    });
    publication.until_held().await;
    let new_runtime = runtime.clone();
    let new_caller = fixture.caller.clone();
    let new = tokio::spawn(async move {
        new_runtime
            .request_central(&new_caller, new_request)
            .await
            .unwrap()
    });
    runtime.runtime().until_central_waiting(1).await;
    assert!(!new.is_finished());
    publication.release();
    let Result::Done(Answer::WakeSet {
        wake_id: later_id, ..
    }) = old.await.unwrap()
    else {
        panic!("later wake refused")
    };
    let Result::Done(Answer::WakeSet {
        wake_id: earlier_id,
        ..
    }) = new.await.unwrap()
    else {
        panic!("earlier wake refused")
    };
    assert_eq!(
        runtime
            .runtime()
            .statement_host()
            .alarm()
            .unwrap()
            .unix_millis(),
        earlier.timestamp_millis()
    );
    let mut timer_commit = runtime
        .runtime()
        .pause_points()
        .hold_central(fleet_runtime::PausePoint::AfterTimerCommit);
    let completed = runtime.runtime().alarm_completion_count();
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    fixture.advance(Duration::from_secs(300)).await;
    timer_commit.until_held().await;
    let rows = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT wake_id,state FROM automation_wakes ORDER BY due_at",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![
            vec![text(earlier_id), text("done")],
            vec![text(later_id), text("active")]
        ]
    );
    let rows = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![central_storage::SqlValue::Integer(
            1.try_into().unwrap()
        )]]
    );
    // Embedded (Lead ruling): due units commit with the alarm unchanged and the
    // re-arm is the event's final step, so the same oracle is read after it.
    timer_commit.release();
    runtime.runtime().until_alarm_completed(completed + 1).await;
    assert_eq!(
        runtime
            .runtime()
            .statement_host()
            .alarm()
            .unwrap()
            .unix_millis(),
        later.timestamp_millis()
    );
    fixture.host.close().await;
}
