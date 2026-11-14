//! Corrupt persisted cadence and timer horizons fail at the real request boundary.
use super::*;

pub(super) async fn exercise_stored_validation(f: &Fixture) {
    use central_storage::{
        AlarmUpdate, CorruptReason, GuardedStatement, RowExpectation, StatementBatch, StoreFailure,
    };
    let r = f.host.central(&f.caller.organization_id).await;
    let actor = r
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id FROM actor_identities WHERE identity LIKE '%cadence-2%'",
            args: vec![],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone();
    for (number, sql, value, field) in [
        (
            42100,
            "UPDATE actor_identities SET notice_cadence=? WHERE actor_identity_id=?",
            "m20",
            "actor_identities.notice_cadence",
        ),
        (
            42101,
            "UPDATE actor_identities SET last_notice_delivery_at=? WHERE actor_identity_id=?",
            "2020-01-01T00:01:00Z",
            "actor_identities.last_notice_delivery_at",
        ),
    ] {
        let before=r.statement_host().read(Statement{sql:"SELECT notice_cadence,last_notice_delivery_at FROM actor_identities WHERE actor_identity_id=?",args:vec![actor.clone()]}).await.unwrap();
        assert!(matches!(
            r.statement_host()
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
                &f.caller,
                CentralMailRequest {
                    request_id: id(number),
                    operation: Op::SendMessage {
                        to: session(&f.caller),
                        body: body(),
                    },
                },
            )
            .await;
        assert!(
            matches!(result,Err(StoreFailure::Corrupt{reason:CorruptReason::BadValue{column}}) if column==field)
        );
        assert!(matches!(r.statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"UPDATE actor_identities SET notice_cadence=?,last_notice_delivery_at=? WHERE actor_identity_id=?",args:vec![before.rows[0][0].clone(),before.rows[0][1].clone(),actor.clone()]},expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
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
        r.statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE wakes SET due_at=? WHERE wake_id=?",
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
            &f.caller,
            CentralMailRequest {
                request_id: id(42111),
                operation: Op::CancelWake {
                    wake_id: wake_id.clone(),
                },
            },
        )
        .await;
    assert!(
        matches!(invalid,Err(StoreFailure::Corrupt{reason:CorruptReason::BadValue{column}}) if column=="wakes.due_at")
    );
    assert!(matches!(
        r.statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE wakes SET due_at=? WHERE wake_id=?",
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
        AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, RowExpectation, StatementBatch,
        StoreFailure,
    };
    // Keep the newly admitted interval's first mark ahead of the driver while corrupting rows.
    advance_to(f, now(f) + chrono::Duration::seconds(1));
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
    let message = request(
        f,
        &f.caller,
        42122,
        Op::SendMessage {
            to: session(&f.caller),
            body: body(),
        },
    )
    .await;
    let Result::Done(Answer::Queued { message_id }) = message else {
        panic!("stored message setup: {message:?}")
    };
    let runtime = f.host.central(&f.caller.organization_id).await;
    for (index, select, update, row_id, invalid_value, column) in [
        (
            0,
            "SELECT expires_at FROM automations WHERE automation_id=?",
            "UPDATE automations SET expires_at=? WHERE automation_id=?",
            text(&automation_id),
            text((now(f) + chrono::Duration::days(8)).to_rfc3339()),
            "automations.expires_at",
        ),
        (
            1,
            "SELECT anchor_at FROM automations WHERE automation_id=?",
            "UPDATE automations SET anchor_at=? WHERE automation_id=?",
            text(&automation_id),
            text((anchor - chrono::Duration::seconds(300)).to_rfc3339()),
            "automations.anchor_at",
        ),
        (
            2,
            "SELECT next_due_at FROM automations WHERE automation_id=?",
            "UPDATE automations SET next_due_at=? WHERE automation_id=?",
            text(&automation_id),
            // This remains on the grid and within the horizon, but is not a 600-second occurrence.
            text((anchor + chrono::Duration::seconds(300)).to_rfc3339()),
            "automations.next_due_at",
        ),
        (
            3,
            "SELECT created_at FROM wakes WHERE wake_id=?",
            "UPDATE wakes SET created_at=? WHERE wake_id=?",
            text(&wake_id),
            text("not-a-timestamp"),
            "wakes.created_at",
        ),
        (
            4,
            "SELECT deliver_after FROM messages WHERE message_id=?",
            "UPDATE messages SET deliver_after=? WHERE message_id=?",
            text(&message_id),
            text("not-a-timestamp"),
            "messages.deliver_after",
        ),
    ] {
        let original = runtime
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
                .statement_host()
                .run_batch(batch(invalid_value.clone()))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        let snapshot = || Statement {
            sql: "SELECT (SELECT COUNT(*) FROM automations),(SELECT COUNT(*) FROM wakes),(SELECT COUNT(*) FROM messages),(SELECT json_group_array(json_array(request_id,answer,created_at,updated_at)) FROM request_log)",
            args: vec![],
        };
        let before = runtime
            .statement_host()
            .read(snapshot())
            .await
            .unwrap()
            .rows;
        let alarm_before = runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap();
        let result = f
            .host
            .request_central(
                &f.caller,
                CentralMailRequest {
                    request_id: id(42130 + index),
                    operation: Op::SendMessage {
                        to: session(&f.caller),
                        body: body(),
                    },
                },
            )
            .await;
        assert_eq!(
            result,
            Err(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: column.to_owned()
                },
            })
        );
        assert_eq!(
            runtime
                .statement_host()
                .read(snapshot())
                .await
                .unwrap()
                .rows,
            before
        );
        assert_eq!(
            runtime
                .control::<Option<i64>>("raw-alarm-time", &())
                .await
                .unwrap(),
            alarm_before
        );
        assert_eq!(
            runtime
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
                .statement_host()
                .run_batch(batch(original.rows[0][0].clone()))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        assert_eq!(
            runtime
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
