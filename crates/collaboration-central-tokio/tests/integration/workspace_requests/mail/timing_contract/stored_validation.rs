//! Corrupt persisted cadence and timer horizons fail at the real request boundary.
use super::*;

pub(super) async fn exercise_stored_validation(f: &Fixture) {
    use central_storage::{
        AlarmUpdate, CorruptReason, GuardedStatement, RowExpectation, StatementBatch, StoreFailure,
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
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
}
