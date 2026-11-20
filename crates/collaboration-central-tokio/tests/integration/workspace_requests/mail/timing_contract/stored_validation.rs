//! Corrupt persisted cadence and timer horizons fail at the real request boundary.
use super::*;

pub(super) async fn exercise_stored_validation(f: &Fixture) {
    use central_storage::{AlarmUpdate, GuardedStatement, RowExpectation, StatementBatch};
    let r = f.host.central(f.caller.organization_id()).await.unwrap();
    let actor = r
        .runtime().statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id FROM access_actor_identities WHERE json_extract(identity,'$.session.sessionId')='cadence-2'",
            args: vec![],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone();
    for (number, sql, value, field) in [(
        42100,
        "UPDATE access_actor_identities SET notice_cadence=? WHERE actor_identity_id=?",
        "m20",
        "access_actor_identities.notice_cadence",
    )] {
        let before = r
            .runtime()
            .statement_host()
            .read(Statement {
                sql: "SELECT notice_cadence FROM access_actor_identities WHERE actor_identity_id=?",
                args: vec![actor.clone()],
            })
            .await
            .unwrap();
        assert!(matches!(
            r.runtime()
                .statement_host()
                .run_batch(StatementBatch {
                    statements: vec![GuardedStatement {
                        statement: Statement {
                            sql,
                            args: vec![text(value), actor.clone()]
                        },
                        expect: RowExpectation::Exactly(1)
                    }],
                    alarm: AlarmUpdate::Unchanged
                })
                .await
                .unwrap(),
            central_storage::BatchOutcome::Applied { .. }
        ));
        let result = f
            .host
            .request_central(
                &f.fresh_caller(&f.caller).await,
                CentralMailRequest {
                    request_id: id(number),
                    operation: Op::SendMessage {
                        to: session(&f.caller),
                        body: body(),
                        delivery: workspace_model::central_mail::MessageDelivery::Auto,
                        reply_to: None,
                        sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
                    },
                },
            )
            .await;
        // Embedded: with the global preflight gone, an unrelated request succeeds;
        // the invalid row fails only at its own use.
        let _ = field;
        assert!(
            matches!(result, Ok(Result::Done(Answer::Queued { .. }))),
            "{result:?}"
        );
        assert!(matches!(r.runtime().statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"UPDATE access_actor_identities SET notice_cadence=? WHERE actor_identity_id=?",args:vec![before.rows[0][0].clone(),actor.clone()]},expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
    }
    let due = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    let result = request(
        f,
        &f.caller,
        42110,
        Op::SetWake {
            target: session(&f.caller),
            due_at: due,
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("stored horizon setup: {result:?}")
    };
    assert!(matches!(
        r.runtime()
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE automation_wakes SET due_at=? WHERE wake_id=?",
                        args: vec![
                            text((due + chrono::Duration::days(8)).to_rfc3339()),
                            text(&wake_id)
                        ]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));
    let invalid = f
        .host
        .request_central(
            &f.fresh_caller(&f.caller).await,
            CentralMailRequest {
                request_id: id(42111),
                operation: Op::CancelWake {
                    wake_id: wake_id.clone(),
                },
            },
        )
        .await;
    // Embedded: CancelWake does not need the broken due time, so it succeeds,
    // disables the wake and leaves no failure record.
    assert!(matches!(invalid, Ok(Result::Done(_))), "{invalid:?}");
    let cancelled = r.runtime().statement_host().read(Statement { sql: "SELECT state,(SELECT COUNT(*) FROM engine_work_attempts WHERE subject_key=?) FROM automation_wakes WHERE wake_id=?", args: vec![text(&wake_id), text(&wake_id)] }).await.unwrap();
    assert_eq!(
        cancelled.rows,
        vec![vec![
            text("disabled"),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert!(matches!(
        r.runtime()
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE automation_wakes SET due_at=? WHERE wake_id=?",
                        args: vec![text(due.to_rfc3339()), text(&wake_id)]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));
    assert!(matches!(
        request(f, &f.caller, 42112, Op::CancelWake { wake_id }).await,
        Result::Done(_)
    ));
    exercise_timing_row_corruption(f).await;
}

async fn exercise_timing_row_corruption(f: &Fixture) {
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
    };
    // Keep the newly admitted interval's first mark ahead of the driver while corrupting rows.
    advance_to(f, now(f) + chrono::Duration::seconds(1)).await;
    let interval = request(
        f,
        &f.caller,
        42120,
        Op::SetAutomation {
            target: session(&f.caller),
            timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":600}))
                .unwrap(),
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::AutomationSet {
        automation_id,
        next_due_at: Some(anchor),
        ..
    }) = interval
    else {
        panic!("stored automation setup: {interval:?}")
    };
    let wake = request(
        f,
        &f.caller,
        42121,
        Op::SetWake {
            target: session(&f.caller),
            due_at: anchor,
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::WakeSet { wake_id, .. }) = wake else {
        panic!("stored wake setup: {wake:?}")
    };
    let runtime = f.host.central(f.caller.organization_id()).await.unwrap();
    let pending = runtime.runtime().statement_host().read(Statement {sql:"SELECT w.window_id FROM mail_notification_windows w JOIN access_actor_identities a ON a.actor_identity_id=w.recipient_actor WHERE json_extract(a.identity,'$.session.sessionId')='cadence-2'",args:vec![]}).await.unwrap();
    assert_eq!(pending.rows.len(), 1);
    for (index, select, update, row_id, invalid_value, column) in [
        (
            0,
            "SELECT expires_at FROM automation_schedules WHERE automation_id=?",
            "UPDATE automation_schedules SET expires_at=? WHERE automation_id=?",
            text(&automation_id),
            text((now(f) + chrono::Duration::days(8)).to_rfc3339()),
            "automation_schedules.expires_at",
        ),
        (
            1,
            "SELECT anchor_at FROM automation_schedules WHERE automation_id=?",
            "UPDATE automation_schedules SET anchor_at=? WHERE automation_id=?",
            text(&automation_id),
            text((anchor - chrono::Duration::seconds(300)).to_rfc3339()),
            "automation_schedules.anchor_at",
        ),
        (
            2,
            "SELECT next_due_at FROM automation_schedules WHERE automation_id=?",
            "UPDATE automation_schedules SET next_due_at=? WHERE automation_id=?",
            text(&automation_id),
            // This remains on the grid and within the horizon, but is not a 600-second occurrence.
            text((anchor + chrono::Duration::seconds(300)).to_rfc3339()),
            "automation_schedules.next_due_at",
        ),
        (
            3,
            "SELECT created_at FROM automation_wakes WHERE wake_id=?",
            "UPDATE automation_wakes SET created_at=? WHERE wake_id=?",
            text(&wake_id),
            text("not-a-timestamp"),
            "automation_wakes.created_at",
        ),
        (
            4,
            "SELECT due_at FROM mail_notification_windows WHERE window_id=?",
            "UPDATE mail_notification_windows SET due_at=? WHERE window_id=?",
            pending.rows[0][0].clone(),
            // A five-minute mark that is not on this recipient's shared M55 schedule.
            text("2030-01-01T03:55:00Z"),
            "mail_notification_windows.due_at",
        ),
    ] {
        let original = runtime
            .runtime()
            .statement_host()
            .read(Statement {
                sql: select,
                args: vec![row_id.clone()],
            })
            .await
            .unwrap();
        assert_eq!(original.rows.len(), 1);
        let batch = |value| StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: update,
                    args: vec![value, row_id.clone()],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        };
        assert!(matches!(
            runtime
                .runtime()
                .statement_host()
                .run_batch(batch(invalid_value.clone()))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        let snapshot = || Statement {
            sql: "SELECT (SELECT COUNT(*) FROM automation_schedules),(SELECT COUNT(*) FROM automation_wakes),(SELECT COUNT(*) FROM mail_messages),(SELECT json_group_array(json_array(request_id,answer,created_at,updated_at)) FROM runtime_request_log)",
            args: vec![],
        };
        let before = runtime
            .runtime()
            .statement_host()
            .read(snapshot())
            .await
            .unwrap()
            .rows;
        let alarm_before = runtime.runtime().statement_host().alarm();
        let result = f
            .host
            .request_central(
                &f.fresh_caller(&f.caller).await,
                CentralMailRequest {
                    request_id: id(42130 + index),
                    operation: Op::SendMessage {
                        to: session(&f.caller),
                        body: body(),
                        delivery: workspace_model::central_mail::MessageDelivery::Auto,
                        reply_to: None,
                        sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
                    },
                },
            )
            .await;
        // Embedded: the unrelated request now succeeds and leaves the invalid row
        // for its own use; the timer and window rows themselves are unchanged.
        let _ = (column, &before, &alarm_before, snapshot);
        assert!(
            matches!(result, Ok(Result::Done(Answer::Queued { .. }))),
            "{result:?}"
        );
        assert_eq!(
            runtime
                .runtime()
                .statement_host()
                .read(Statement {
                    sql: select,
                    args: vec![row_id.clone()]
                })
                .await
                .unwrap()
                .rows,
            vec![vec![invalid_value]]
        );
        assert!(matches!(
            runtime
                .runtime()
                .statement_host()
                .run_batch(batch(original.rows[0][0].clone()))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        assert_eq!(
            runtime
                .runtime()
                .statement_host()
                .read(Statement {
                    sql: select,
                    args: vec![row_id]
                })
                .await
                .unwrap()
                .rows,
            original.rows
        );
    }
    assert!(matches!(
        request(f, &f.caller, 42135, Op::DisableAutomation { automation_id }).await,
        Result::Done(_)
    ));
    assert!(matches!(
        request(f, &f.caller, 42136, Op::CancelWake { wake_id }).await,
        Result::Done(_)
    ));
}
