use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};
use router_clock::RuntimeClock;

async fn execute(fixture: &Fixture, sql: &'static str, args: Vec<central_storage::SqlValue>) {
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let outcome = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
}

#[tokio::test]
async fn corrupt_timer_target_and_timing_fail_alarm_without_effects() {
    for (number, corrupt_sql, expected_column) in [
        (1300, "UPDATE wakes SET target='not-json'", "wakes.target"),
        (
            1301,
            "UPDATE automations SET timing_rule='not-json'",
            "automations.timing_rule",
        ),
    ] {
        let fixture = Fixture::new().await;
        let runtime = fixture
            .host
            .central(&fixture.caller.organization_id)
            .await
            .unwrap();
        let due = fixture.clock.utc_now() + chrono::Duration::seconds(300);
        let target = session(&fixture.caller);
        let operation = if number == 1300 {
            Operation::SetWake {
                target,
                due_at: due,
                text: "wake".to_owned().try_into().unwrap(),
            }
        } else {
            Operation::SetAutomation {
                target,
                timing: serde_json::from_str(r#"{"kind":"after","seconds":300}"#).unwrap(),
                text: "automation".to_owned().try_into().unwrap(),
            }
        };
        assert!(matches!(
            fixture
                .host
                .request_central(
                    &fixture.caller,
                    CentralMailRequest {
                        request_id: id(number),
                        operation
                    }
                )
                .await
                .unwrap(),
            Result::Done(_)
        ));
        let prior_alarm = runtime
            .statement_host()
            .alarm()
            .map(|value| value.unix_millis());
        assert_eq!(prior_alarm, Some(due.timestamp_millis()));
        execute(&fixture, corrupt_sql, vec![]).await;
        fixture.clock.set_utc(due);
        let error = runtime.on_alarm().await.unwrap_err();
        assert!(
            matches!(error, central_storage::StoreFailure::Corrupt { reason: central_storage::CorruptReason::BadValue { column } } if column == expected_column)
        );

        let rows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM wakes WHERE state='active'),(SELECT COUNT(*) FROM automations WHERE state='active'),(SELECT COUNT(*) FROM messages)", args: vec![] }).await.unwrap();
        let (wakes, automations) = if number == 1300 { (1, 0) } else { (0, 1) };
        assert_eq!(
            rows.rows,
            vec![vec![
                central_storage::SqlValue::Integer(wakes.try_into().unwrap()),
                central_storage::SqlValue::Integer(automations.try_into().unwrap()),
                central_storage::SqlValue::Integer(0.try_into().unwrap())
            ]]
        );
        assert_eq!(
            runtime
                .statement_host()
                .alarm()
                .map(|value| value.unix_millis()),
            prior_alarm
        );
        fixture.host.close().await;
    }
}

#[tokio::test]
async fn later_timer_statement_failure_rolls_back_queue_lifecycle_and_alarm() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::seconds(300);
    let operation = Operation::SetWake {
        target: session(&fixture.caller),
        due_at: due,
        text: "wake".to_owned().try_into().unwrap(),
    };
    let Result::Done(Answer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1310),
                operation,
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    let prior_alarm = runtime
        .statement_host()
        .alarm()
        .map(|value| value.unix_millis());
    assert_eq!(prior_alarm, Some(due.timestamp_millis()));
    execute(&fixture, "CREATE TRIGGER reject_wake_finish BEFORE UPDATE ON wakes BEGIN SELECT RAISE(ABORT,'reject wake finish'); END", vec![]).await;
    fixture.clock.set_utc(due);
    assert!(runtime.on_alarm().await.is_err());
    let rows = runtime.statement_host().read(Statement { sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake') FROM wakes WHERE wake_id=?", args: vec![text(&wake_id)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("active"),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|value| value.unix_millis()),
        prior_alarm
    );
    execute(&fixture, "DROP TRIGGER reject_wake_finish", vec![]).await;
    runtime.on_alarm().await.unwrap();
    assert_eq!(runtime.on_alarm().await.unwrap().wakes_fired, 0);
    let rows = runtime.statement_host().read(Statement { sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake') FROM wakes WHERE wake_id=?", args: vec![text(wake_id)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("done"),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|value| value.unix_millis()),
        None
    );
    fixture.host.close().await;
}
