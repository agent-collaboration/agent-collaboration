use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

async fn execute(fixture: &Fixture, sql: &'static str, args: Vec<SqlValue>) {
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args },
                    expect: RowExpectation::Any
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}

#[tokio::test]
async fn bad_due_window_does_not_suppress_wakes_or_automations() {
    let fixture = Fixture::new().await;
    let area = fixture.area(8210).await;
    let thread = fixture.thread(8211, area).await;
    let (_, recipient) = joined_observer(&fixture, &thread).await;
    created(fixture.call(8212, post(&thread, vec![])).await);
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let window = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT window_id,due_at FROM mail_notification_windows WHERE recipient_actor=?",
            args: vec![text(&recipient)],
        })
        .await
        .unwrap();
    let [SqlValue::Text(window_id), SqlValue::Text(due)] = window.rows[0].as_slice() else {
        panic!("window shape")
    };
    let due = chrono::DateTime::parse_from_rfc3339(due)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8213),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "healthy wake".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    let CentralMailResult::Done(CentralMailAnswer::AutomationSet { automation_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8216),
                operation: CentralMailOperation::SetAutomation {
                    target: session(&fixture.caller),
                    timing: serde_json::from_value(serde_json::json!({"kind":"at","at":due}))
                        .unwrap(),
                    text: "healthy automation".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("automation refused")
    };
    let items = runtime.statement_host().read(Statement {
        sql: "SELECT window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at FROM mail_notification_window_items WHERE window_id=?", args: vec![text(window_id)],
    }).await.unwrap();
    assert!(!items.rows.is_empty());
    execute(
        &fixture,
        "DELETE FROM mail_notification_window_items WHERE window_id=?",
        vec![text(window_id)],
    )
    .await;
    fixture.set_utc(due).await;

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a bad window must not fail timers: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT (SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='automation' AND json_extract(body,'$.automationId')=?),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='notice')", args: vec![text(&wake_id), text(&automation_id)],
    }).await.unwrap();
    // The other-window half of the branch test is deferred: today's whole unit rolls back.
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    for count in 1..=3 {
        if count > 1 {
            fixture
                .set_utc(due + chrono::Duration::minutes(5 * (count - 1)))
                .await;
            runtime.after_event().await.unwrap();
        }
        let record = runtime.statement_host().read(Statement {
            sql: "SELECT state,failure_count FROM engine_work_attempts WHERE machine_kind='legacy_notice_windows' AND subject_key='all'", args: vec![],
        }).await.unwrap();
        assert_eq!(
            record.rows,
            vec![vec![
                text("failing"),
                SqlValue::Integer(count.try_into().unwrap())
            ]]
        );
    }
    for row in items.rows {
        execute(&fixture, "INSERT INTO mail_notification_window_items(window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?)", row).await;
    }
    fixture.set_utc(due + chrono::Duration::minutes(15)).await;
    runtime.after_event().await.unwrap();
    let record = runtime.statement_host().read(Statement {
        sql: "SELECT (SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='legacy_notice_windows' AND subject_key='all'),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='notice' AND recipient_actor=?)", args: vec![text(&recipient)],
    }).await.unwrap();
    assert_eq!(
        record.rows,
        vec![vec![
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn corrupt_drain_moves_retry_into_future_without_blocking_due_wake() {
    let fixture = Fixture::new().await;
    let area = fixture.area(8220).await;
    let thread = fixture.thread(8221, area).await;
    created(fixture.call(8223, post(&thread, vec![])).await);
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8222),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "healthy wake".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let drains = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT workspace_id FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(drains.rows.len(), 1);
    execute(
        &fixture,
        "UPDATE mail_workspace_drains SET drain_due_at='not-a-timestamp' WHERE workspace_id=?",
        vec![text(&fixture.workspace)],
    )
    .await;
    fixture.set_utc(due).await;

    let outcome = runtime.after_event().await;

    assert!(
        outcome.is_ok(),
        "a broken drain must not fail timers: {outcome:?}"
    );
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?) FROM automation_wakes WHERE wake_id=?", args: vec![text(&wake_id), text(&wake_id)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![text("done"), SqlValue::Integer(1.try_into().unwrap())]]
    );
    for count in 1..=3 {
        if count > 1 {
            fixture
                .set_utc(due + chrono::Duration::minutes(5 * (count - 1)))
                .await;
            runtime.after_event().await.unwrap();
        }
        let record = runtime.statement_host().read(Statement {
            sql: "SELECT state,failure_count,next_try_at FROM engine_work_attempts WHERE machine_kind='legacy_workspace_drains' AND subject_key='all'", args: vec![],
        }).await.unwrap();
        assert_eq!(
            record.rows,
            vec![vec![
                text("failing"),
                SqlValue::Integer(count.try_into().unwrap()),
                text(
                    (due + chrono::Duration::minutes(count * 5))
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                )
            ]]
        );
    }
    execute(
        &fixture,
        "UPDATE mail_workspace_drains SET drain_due_at=? WHERE workspace_id=?",
        vec![
            text(due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            text(&fixture.workspace),
        ],
    )
    .await;
    fixture.set_utc(due + chrono::Duration::minutes(15)).await;
    runtime.after_event().await.unwrap();
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='legacy_workspace_drains' AND subject_key='all'", args: vec![],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![SqlValue::Integer(0.try_into().unwrap())]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn request_drains_obey_the_unit_gate_and_one_workspace_success_keeps_its_record() {
    let fixture = Fixture::new().await;
    let area = fixture.area(8310).await;
    let thread = fixture.thread(8311, area).await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let now = fixture.clock.utc_now();
    let stamp = |at: m::Timestamp| text(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let attempt_id: m::RequestId = id(8312);
    execute(&fixture,
        "INSERT INTO engine_work_attempts VALUES (?,'legacy_workspace_drains','all','failing',1,?,'statement_rejected',NULL,?,?,NULL,?,?)",
        vec![text(&attempt_id),stamp(now+chrono::Duration::minutes(5)),stamp(now),stamp(now),stamp(now),stamp(now)]).await;
    let before = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap();

    created(fixture.call(8313, post(&thread, vec![])).await);

    let deferred = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(
        deferred.rows, before.rows,
        "future retry must skip the immediate request drain"
    );
    execute(&fixture,"UPDATE engine_work_attempts SET state='retired',next_try_at=NULL,retired_at=? WHERE attempt_id=?",vec![stamp(now),text(&attempt_id)]).await;
    created(fixture.call(8314, post(&thread, vec![])).await);
    let retired = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(
        retired.rows, before.rows,
        "traffic must leave a retired drain stopped"
    );
    execute(&fixture,"UPDATE engine_work_attempts SET state='failing',next_try_at=?,retired_at=NULL WHERE attempt_id=?",vec![stamp(now),text(&attempt_id)]).await;

    created(fixture.call(8315, post(&thread, vec![])).await);

    let completed = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM mail_workspace_drains WHERE workspace_id=?",
            args: vec![text(&fixture.workspace)],
        })
        .await
        .unwrap();
    assert_ne!(
        completed.rows, before.rows,
        "a reached retry permits the immediate drain"
    );
    let record = runtime.statement_host().read(Statement {
        sql: "SELECT attempt_id,state,failure_count,last_failure FROM engine_work_attempts WHERE machine_kind='legacy_workspace_drains' AND subject_key='all'", args: vec![],
    }).await.unwrap();
    assert_eq!(
        record.rows,
        vec![vec![
            text(&attempt_id),
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text("statement_rejected")
        ]],
        "one workspace cannot clear the whole unit's record"
    );
    fixture.host.close().await;
}
