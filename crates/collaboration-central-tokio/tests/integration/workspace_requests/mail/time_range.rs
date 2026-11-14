use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};

fn wake(target: workspace_model::SessionAddress, due_at: workspace_model::Timestamp) -> Operation {
    Operation::SetWake {
        target,
        due_at,
        text: "range".to_owned().try_into().unwrap(),
    }
}

#[tokio::test]
async fn timing_contract_refuses_off_grid_wake_and_seven_day_overflow() {
    use router_clock::RuntimeClock;
    let fixture = Fixture::new().await;
    let now = fixture.clock.utc_now();
    for (index, offset) in [60, 301, 604_800 + 300].into_iter().enumerate() {
        let result = fixture
            .host
            .request_central(
                &fixture.caller,
                CentralMailRequest {
                    request_id: id(1800 + u32::try_from(index).unwrap()),
                    operation: wake(
                        session(&fixture.caller),
                        now + chrono::Duration::seconds(offset),
                    ),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            result,
            Result::Refused(Refusal::TimingViolation {
                field: "dueAt".to_owned(),
                requirement: if offset > 604_800 {
                    collaboration_protocol::central_mail::TimingRequirement::WithinSevenDays
                } else {
                    collaboration_protocol::central_mail::TimingRequirement::FiveMinuteGrid
                },
            }),
            "invalid offset {offset}"
        );
    }
    fixture.host.close().await;
}

#[tokio::test]
async fn out_of_range_grid_and_horizon_refusals_write_nothing_and_seven_days_is_accepted() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let counts_query = || Statement {
        sql: "SELECT (SELECT COUNT(*) FROM messages),(SELECT COUNT(*) FROM wakes),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM actor_identities)",
        args: vec![],
    };
    let before = runtime
        .statement_host()
        .read(counts_query())
        .await
        .unwrap()
        .rows;
    let mut new_caller = fixture.caller.clone();
    let mut new_session = session(&fixture.caller);
    new_session.session_id = "range-new-caller".to_owned().try_into().unwrap();
    new_caller.caller = workspace_model::Caller::Session(new_session);
    let invalid = chrono::DateTime::from_timestamp_millis(
        collaboration_central_core::MAX_ALARM_UNIX_MILLIS + 1000,
    )
    .unwrap();
    let result = fixture
        .host
        .request_central(
            &new_caller,
            CentralMailRequest {
                request_id: id(1600),
                operation: wake(session(&fixture.caller), invalid),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        result,
        Result::Refused(Refusal::TimeOutOfRange {
            field: "dueAt".to_owned()
        })
    );
    assert_eq!(
        runtime
            .statement_host()
            .read(counts_query())
            .await
            .unwrap()
            .rows,
        before
    );
    let ceiling =
        chrono::DateTime::from_timestamp_millis(collaboration_central_core::MAX_ALARM_UNIX_MILLIS)
            .unwrap();
    let last_grid =
        chrono::DateTime::from_timestamp(ceiling.timestamp().div_euclid(300) * 300, 0).unwrap();
    for (number, due, requirement) in [
        (
            1602,
            ceiling,
            collaboration_protocol::central_mail::TimingRequirement::FiveMinuteGrid,
        ),
        (
            1603,
            last_grid,
            collaboration_protocol::central_mail::TimingRequirement::WithinSevenDays,
        ),
    ] {
        let refused = fixture
            .host
            .request_central(
                &new_caller,
                CentralMailRequest {
                    request_id: id(number),
                    operation: wake(session(&fixture.caller), due),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            refused,
            Result::Refused(Refusal::TimingViolation {
                field: "dueAt".to_owned(),
                requirement
            })
        );
        assert_eq!(
            runtime
                .statement_host()
                .read(counts_query())
                .await
                .unwrap()
                .rows,
            before
        );
    }
    let last =
        router_clock::RuntimeClock::utc_now(fixture.clock.as_ref()) + chrono::Duration::days(7);
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1601),
                operation: wake(session(&fixture.caller), last),
            },
        )
        .await
        .unwrap();
    assert!(matches!(result, Result::Done(Answer::WakeSet { due_at, .. }) if due_at == last));
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state,due_at,(SELECT COUNT(*) FROM messages) FROM wakes",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("active"),
            text(last.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        Some(last.timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn unconvertible_stored_wake_due_fails_closed_on_actual_alarm() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let due = router_clock::RuntimeClock::utc_now(fixture.clock.as_ref())
        + chrono::Duration::seconds(300);
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1610),
                operation: wake(session(&fixture.caller), due),
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    let prior_alarm = runtime
        .statement_host()
        .alarm()
        .map(|time| time.unix_millis());
    assert_eq!(prior_alarm, Some(due.timestamp_millis()));
    let outcome = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE wakes SET due_at='+10000-01-01T00:00:00Z' WHERE wake_id=?",
                    args: vec![text(&wake_id)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
    let error = runtime.on_alarm().await.unwrap_err();
    assert!(
        matches!(error, central_storage::StoreFailure::Corrupt { reason: central_storage::CorruptReason::BadValue { column } } if column == "wakes.due_at")
    );
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state,due_at,(SELECT COUNT(*) FROM messages) FROM wakes WHERE wake_id=?",
            args: vec![text(wake_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("active"),
            text("+10000-01-01T00:00:00Z"),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        prior_alarm
    );
    fixture.host.close().await;
}
