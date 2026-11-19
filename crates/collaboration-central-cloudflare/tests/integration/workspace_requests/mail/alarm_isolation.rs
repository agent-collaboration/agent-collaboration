//! Isolation regressions, first exercised against main e7b709f6a.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use std::sync::atomic::Ordering;
fn now(fixture: &Fixture) -> workspace_model::Timestamp {
    chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap()
}
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
                    due_at: now(fixture) + chrono::Duration::minutes(5),
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
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
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
    let due = now(&fixture) + chrono::Duration::minutes(5);
    fixture
        .clock
        .0
        .store(due.timestamp_millis(), Ordering::SeqCst);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a broken item must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM automation_wakes WHERE wake_id=?",
        args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let failure = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure,last_failure_detail FROM engine_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?",
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
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some((due + chrono::Duration::minutes(5)).timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn corrupt_due_wake_target_does_not_block_other_timers() {
    exercise_isolation(
        "UPDATE automation_wakes SET target='not-json' WHERE wake_id=?",
        "automation_wakes.target",
    )
    .await;
}

#[tokio::test]
async fn unreadable_due_wake_does_not_block_other_due_wake() {
    exercise_isolation(
        "UPDATE automation_wakes SET due_at='not-a-time' WHERE wake_id=?",
        "automation_wakes.due_at",
    )
    .await;
}

#[tokio::test]
async fn unknown_wake_state_does_not_block_other_due_wake() {
    exercise_isolation(
        "UPDATE automation_wakes SET state='damaged' WHERE wake_id=?",
        "automation_wakes.state",
    )
    .await;
}

#[tokio::test]
async fn one_failed_wake_write_does_not_block_other_timers() {
    let fixture = Fixture::new().await;
    let bad = set_wake(&fixture, 3030, "broken").await;
    let healthy = set_wake(&fixture, 3031, "healthy").await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let rejected = runtime.statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement { sql: "CREATE TRIGGER reject_one_wake BEFORE UPDATE ON automation_wakes WHEN OLD.text='broken' BEGIN SELECT RAISE(ABORT,'reject one wake'); END", args: vec![] }, expect: RowExpectation::Any,
        }], alarm: AlarmUpdate::Unchanged,
    }).await.unwrap();
    assert!(matches!(rejected, BatchOutcome::Applied { .. }));
    let due = now(&fixture) + chrono::Duration::minutes(5);
    fixture
        .clock
        .0
        .store(due.timestamp_millis(), Ordering::SeqCst);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a rejected item statement must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM automation_wakes WHERE wake_id=?", args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM engine_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?", args: vec![text(&bad)],
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
    fixture.clock.0.store(
        (due + chrono::Duration::minutes(5)).timestamp_millis(),
        Ordering::SeqCst,
    );
    runtime.after_event().await.unwrap();
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?),(SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?) FROM automation_wakes WHERE wake_id=?", args: vec![text(&bad), text(&bad), text(&bad)],
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
async fn corrupt_due_automation_timing_does_not_block_other_timers() {
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
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE automation_schedules SET timing_rule='not-json' WHERE automation_id=?",
                    args: vec![text(&automation_id)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    let due = now(&fixture) + chrono::Duration::minutes(5);
    fixture
        .clock
        .0
        .store(due.timestamp_millis(), Ordering::SeqCst);

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a broken automation must not fail the alarm: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM automation_wakes WHERE wake_id=?", args: vec![text(&healthy), text(&healthy)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM engine_work_attempts WHERE machine_kind='automation_timer' AND subject_key=?", args: vec![text(&automation_id)],
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
        panic!("wake setup refused: {result:?}")
    };
    wake_id
}

async fn fired_wake_count(fixture: &Fixture, wake: &workspace_ids::WakeId) -> usize {
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let messages = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT body FROM mail_messages WHERE message_kind='wake'",
            args: vec![],
        })
        .await
        .unwrap();
    messages
        .rows
        .iter()
        .filter(|row| {
            let SqlValue::Text(body) = &row[0] else {
                panic!("wake body")
            };
            matches!(
                serde_json::from_str::<workspace_model::central_mail::MessageBody>(body).unwrap(),
                workspace_model::central_mail::MessageBody::Wake { wake_id, .. } if &wake_id == wake
            )
        })
        .count()
}

#[tokio::test]
async fn corrupt_queued_direct_message_does_not_block_timers() {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let start = now(&f);
    let due = start + chrono::Duration::seconds(300);
    let later_due = start + chrono::Duration::seconds(600);
    let healthy = set_wake_at(&f, 8100, due, "healthy due").await;
    let later = set_wake_at(&f, 8101, later_due, "healthy later").await;
    let result = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(8102),
                operation: Operation::SendMessage {
                    to: session(&f.caller),
                    body: "unrelated direct".to_owned().try_into().unwrap(),
                    delivery: workspace_model::central_mail::MessageDelivery::Auto,
                    reply_to: None,
                    sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::Queued { message_id }) = result else {
        panic!("direct setup refused: {result:?}")
    };
    let applied = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE mail_messages SET body='not-json' WHERE message_id=?",
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
        sql: "SELECT * FROM mail_messages WHERE message_id=?",
        args: vec![text(&message_id)],
    };
    let before = runtime.statement_host().read(message_row()).await.unwrap();
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(due.timestamp_millis())
    );
    f.clock.0.store(due.timestamp_millis(), Ordering::SeqCst);

    let work = runtime.after_event().await.unwrap();

    assert_eq!(work.wakes_fired, 1);
    assert_eq!(
        runtime.statement_host().read(message_row()).await.unwrap(),
        before
    );
    assert_eq!(
        fired_wake_count(&f, &healthy).await,
        1,
        "one bad item must not suppress the healthy due wake"
    );
    assert_eq!(
        fired_wake_count(&f, &later).await,
        0,
        "future wake must not fire early"
    );
    // No timer item failed, so no timer carries an attempt record.
    let attempts = runtime.statement_host().read(Statement {
        sql: "SELECT machine_kind,subject_key FROM engine_work_attempts WHERE machine_kind IN ('wake_timer','automation_timer')",
        args: vec![],
    }).await.unwrap();
    assert_eq!(attempts.rows, Vec::<Vec<SqlValue>>::new());
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(later_due.timestamp_millis()),
        "physical alarm must retain the earliest healthy timer"
    );
    f.clock
        .0
        .store(later_due.timestamp_millis(), Ordering::SeqCst);
    runtime.after_event().await.unwrap();
    assert_eq!(
        fired_wake_count(&f, &later).await,
        1,
        "the later healthy timer must also fire"
    );
    assert_eq!(fired_wake_count(&f, &healthy).await, 1);
    f.host.close().await;
}
