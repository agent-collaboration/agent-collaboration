use super::super::workerd_host::{AlarmReceipt, AlarmReceiptResult};
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};

fn wake(target: workspace_ids::SessionRef, due_at: workspace_model::Timestamp) -> Operation {
    Operation::SetWake {
        target,
        due_at,
        text: "range".to_owned().try_into().unwrap(),
    }
}

#[tokio::test]
async fn platform_alarm_accepts_exact_boundary_and_rejects_later_milliseconds() {
    // The pinned workerd source bounds KJ nanoseconds; the real platform result decides:
    // https://github.com/cloudflare/workerd/blob/v1.20261012.1/src/workerd/jsg/value.h#L1218-L1224
    const MAX_MILLIS: i64 = 9_223_372_036_854;
    let fixture = Fixture::new().await;
    let runtime = fixture.host.cold_runtime(&fixture.caller.organization_id);
    for (millis, accepted) in [
        (MAX_MILLIS, true),
        (MAX_MILLIS + 1, false),
        (MAX_MILLIS + 1000, false),
    ] {
        let before: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
        let observed: std::result::Result<Option<i64>, String> =
            runtime.control("raw-probe-alarm", &millis).await.unwrap();
        if accepted {
            assert_eq!(observed, Ok(Some(millis)));
        } else {
            assert!(
                observed.is_err(),
                "platform accepted {millis}: {observed:?}"
            );
        }
        let after: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
        assert_eq!(after, before);
    }
    fixture.host.close().await;
}

#[tokio::test]
async fn out_of_range_grid_and_horizon_refusals_write_nothing_and_seven_days_is_accepted() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let counts_query = || Statement {
        sql: "SELECT (SELECT COUNT(*) FROM mail_messages),(SELECT COUNT(*) FROM automation_wakes),(SELECT COUNT(*) FROM runtime_request_log),(SELECT COUNT(*) FROM access_actor_identities)",
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
            workspace_model::central_mail::TimingRequirement::FiveMinuteGrid,
        ),
        (
            1603,
            last_grid,
            workspace_model::central_mail::TimingRequirement::WithinSevenDays,
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
    let last = chrono::DateTime::from_timestamp_millis(
        fixture.clock.0.load(std::sync::atomic::Ordering::SeqCst),
    )
    .unwrap()
        + chrono::Duration::days(7);
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
            sql: "SELECT state,due_at,(SELECT COUNT(*) FROM mail_messages) FROM automation_wakes",
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
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(last.timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn unconvertible_stored_wake_due_fails_closed_on_actual_alarm() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let due = chrono::DateTime::from_timestamp_millis(
        fixture.clock.0.load(std::sync::atomic::Ordering::SeqCst),
    )
    .unwrap()
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
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    assert_eq!(prior_alarm, Some(due.timestamp_millis()));
    let outcome = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE automation_wakes SET due_at='+10000-01-01T00:00:00Z' WHERE wake_id=?",
                    args: vec![text(&wake_id)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
    // Embedded setup allowance: the fixture's setup claimed the due batch at T0.
    fixture.clock.0.store(
        (due - chrono::Duration::seconds(270)).timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
    // Isolation: that wake fails alone as CorruptRow (due_at); the real alarm
    // handler succeeds and re-arms on the next mark.
    runtime.after_event().await.unwrap();
    let record = runtime.statement_host().read(Statement { sql: "SELECT state,failure_count,last_failure,last_failure_detail FROM engine_work_attempts WHERE machine_kind='wake_timer' AND subject_key=?", args: vec![text(&wake_id)] }).await.unwrap();
    assert_eq!(record.rows.len(), 1);
    assert_eq!(
        &record.rows[0][..3],
        &[
            text("failing"),
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            text("corrupt_row")
        ]
    );
    assert!(
        matches!(&record.rows[0][3], central_storage::SqlValue::Text(detail) if detail.contains("automation_wakes.due_at"))
    );
    let receipt: Option<AlarmReceipt> = runtime.control("raw-alarm-status", &()).await.unwrap();
    let receipt = receipt.expect("actual alarm receipt");
    assert!(matches!(receipt.result, AlarmReceiptResult::Applied { .. }));
    assert_eq!(receipt.alarm_after, prior_alarm);
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state,due_at,(SELECT COUNT(*) FROM mail_messages) FROM automation_wakes WHERE wake_id=?",
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
    fixture.host.close().await;
}
