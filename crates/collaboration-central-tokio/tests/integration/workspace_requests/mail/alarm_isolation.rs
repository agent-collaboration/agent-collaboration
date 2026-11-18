//! Isolation regressions, first exercised against main e7b709f6a.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use router_clock::{ClockInstant, RuntimeClock};
use std::time::Duration;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};

async fn set_wake(fixture: &Fixture, number: u32, label: &str) -> workspace_ids::WakeId {
    let Result::Done(Answer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: Operation::SetWake {
                    target: session(&fixture.caller),
                    due_at: fixture.clock.utc_now() + chrono::Duration::minutes(5),
                    text: label.to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake was refused")
    };
    wake_id
}

async fn exercise_isolation(corruption: &'static str, failure_column: &str) {
    let fixture = Fixture::new().await;
    let bad = set_wake(&fixture, 3000, "broken").await;
    let healthy = set_wake(&fixture, 3001, "healthy").await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let applied = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: corruption,
                    args: vec![text(&bad)],
                },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(applied, BatchOutcome::Applied { .. }));
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    fixture.clock.set_utc(due);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a broken item must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM wakes WHERE wake_id=?",
        args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let failure = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure,last_failure_detail FROM central_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?",
        args: vec![text(&bad)],
    }).await.unwrap();
    assert_eq!(failure.rows.len(), 1);
    assert_eq!(
        &failure.rows[0][..3],
        &[
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text("corrupt_row")
        ]
    );
    assert!(
        matches!(&failure.rows[0][3], SqlValue::Text(detail) if detail.contains(failure_column))
    );
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        Some((due + chrono::Duration::minutes(5)).timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn corrupt_due_wake_target_does_not_block_other_due_wake() {
    exercise_isolation(
        "UPDATE wakes SET target='not-json' WHERE wake_id=?",
        "wakes.target",
    )
    .await;
}

#[tokio::test]
async fn unreadable_due_wake_does_not_block_other_due_wake() {
    exercise_isolation(
        "UPDATE wakes SET due_at='not-a-time' WHERE wake_id=?",
        "wakes.due_at",
    )
    .await;
}

#[tokio::test]
async fn unknown_wake_state_does_not_block_other_due_wake() {
    exercise_isolation(
        "UPDATE wakes SET state='damaged' WHERE wake_id=?",
        "wakes.state",
    )
    .await;
}

#[tokio::test]
async fn one_failed_wake_transaction_does_not_block_other_wakes_and_can_retry() {
    let fixture = Fixture::new().await;
    let bad = set_wake(&fixture, 3030, "broken").await;
    let healthy = set_wake(&fixture, 3031, "healthy").await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let rejected = runtime.statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement { sql: "CREATE TRIGGER reject_one_wake BEFORE UPDATE ON wakes WHEN OLD.text='broken' BEGIN SELECT RAISE(ABORT,'reject one wake'); END", args: vec![] }, expect: RowExpectation::Any,
        }], alarm: AlarmUpdate::Unchanged,
    }).await.unwrap();
    assert!(matches!(rejected, BatchOutcome::Applied { .. }));
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    fixture.clock.set_utc(due);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a rejected item statement must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM wakes WHERE wake_id=?", args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM central_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?", args: vec![text(&bad)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text("statement_rejected")
        ]]
    );
    runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "DROP TRIGGER reject_one_wake",
                    args: vec![],
                },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    fixture.clock.set_utc(due + chrono::Duration::minutes(5));
    runtime.after_event().await.unwrap();
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?),(SELECT COUNT(*) FROM central_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?) FROM wakes WHERE wake_id=?", args: vec![text(&bad), text(&bad), text(&bad)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("done"),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn corrupt_due_automation_timing_does_not_block_due_wake() {
    let fixture = Fixture::new().await;
    let healthy = set_wake(&fixture, 3011, "healthy").await;
    let Result::Done(Answer::AutomationSet { automation_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(3010),
                operation: Operation::SetAutomation {
                    target: session(&fixture.caller),
                    timing: serde_json::from_str(r#"{"kind":"after","seconds":300}"#).unwrap(),
                    text: "broken automation".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("automation refused")
    };
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE automations SET timing_rule='not-json' WHERE automation_id=?",
                    args: vec![text(&automation_id)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    fixture.clock.set_utc(due);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a broken automation must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM wakes WHERE wake_id=?", args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM central_work_attempts WHERE machine_kind='automation_timer' AND subject_key=?", args: vec![text(&automation_id)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text("corrupt_row")
        ]]
    );
    fixture.host.close().await;
}

async fn set_wake_at(
    fixture: &Fixture,
    number: u32,
    due: workspace_model::Timestamp,
    label: &str,
) -> workspace_ids::WakeId {
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: Operation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: label.to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    wake_id
}

async fn wake_state(fixture: &Fixture, wake: &workspace_ids::WakeId) -> Vec<Vec<SqlValue>> {
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM wakes WHERE wake_id=?",
        args: vec![text(wake), text(wake)],
    }).await.unwrap().rows
}

#[tokio::test]
async fn corrupt_queued_message_does_not_block_due_wake() {
    let f = Fixture::new().await;
    let due = f.clock.utc_now() + chrono::Duration::seconds(300);
    let later = due + chrono::Duration::seconds(300);
    let result = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(3020),
                operation: Operation::SendMessage {
                    to: session(&f.caller),
                    body: "unrelated".to_owned().try_into().unwrap(),
                    delivery: workspace_model::central_mail::MessageDelivery::Auto,
                    reply_to: None,
                },
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::Queued { message_id }) = result else {
        panic!("message refused: {result:?}")
    };
    let healthy = set_wake_at(&f, 3021, due, "healthy").await;
    let future = set_wake_at(&f, 3022, later, "future").await;
    let runtime = f.host.central(f.caller.organization_id()).await.unwrap();
    let applied = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE messages SET body='not-json' WHERE message_id=?",
                    args: vec![text(&message_id)],
                },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(applied, BatchOutcome::Applied { .. }));
    let message_row = || Statement {
        sql: "SELECT * FROM messages WHERE message_id=?",
        args: vec![text(&message_id)],
    };
    let before = runtime.statement_host().read(message_row()).await.unwrap();
    f.clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    f.clock.set_utc(due);

    let completion = runtime.after_event().await.unwrap();

    assert_eq!(completion.work.wakes_fired, 1);
    assert_eq!(
        runtime.statement_host().read(message_row()).await.unwrap(),
        before
    );
    assert_eq!(
        wake_state(&f, &healthy).await,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    assert_eq!(
        wake_state(&f, &future).await,
        vec![vec![
            text("active"),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    // No timer item failed, so no timer carries an attempt record.
    let attempts = runtime.statement_host().read(Statement {
        sql: "SELECT machine_kind,subject_key FROM central_work_attempts WHERE machine_kind IN ('wake_timer','automation_timer')",
        args: vec![],
    }).await.unwrap();
    assert_eq!(attempts.rows, Vec::<Vec<SqlValue>>::new());
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        Some(later.timestamp_millis())
    );
    f.host.close().await;
}
