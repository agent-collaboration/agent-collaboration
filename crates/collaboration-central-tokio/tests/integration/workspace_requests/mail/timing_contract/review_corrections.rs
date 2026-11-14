//! Accepted review regressions through the real central owner.
use super::*;
pub(super) async fn exercise_shorter_cadence(f: &Fixture) {
    let mut recipient = f.caller.clone();
    let mut address = session(&recipient);
    address.session_id = "shorter-cadence".to_owned().try_into().unwrap();
    recipient.caller = m::Caller::Session(address.clone());
    assert!(matches!(
        request(
            f,
            &recipient,
            46000,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M55
            }
        )
        .await,
        Result::Done(_)
    ));
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let row=r.statement_host().read(Statement{sql:"SELECT actor_identity_id FROM actor_identities WHERE identity LIKE '%shorter-cadence%'",args:vec![]}).await.unwrap();
    let central_storage::SqlValue::Text(actor) = &row.rows[0][0] else {
        panic!("actor")
    };
    let actor: m::ActorIdentityId = actor.parse().unwrap();
    drop(r);
    let area = f.area(46001).await;
    let thread = f.thread(46002, area).await;
    let post = || m::WorkspaceOperation::ThreadPost {
        thread_id: thread.clone(),
        body: body(),
        addressees: vec![m::Addressee::Actor {
            actor_identity_id: actor.clone(),
        }],
    };
    let start = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    advance_to(f, start);
    created(f.call(46003, post()).await);
    let first = start + chrono::Duration::minutes(55);
    dispatch_to(f, first).await;
    advance_to(f, first + chrono::Duration::minutes(10));
    created(f.call(46004, post()).await);
    advance_to(
        f,
        first + chrono::Duration::minutes(30) + chrono::Duration::seconds(20),
    );
    assert!(matches!(
        request(
            f,
            &recipient,
            46005,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M15
            }
        )
        .await,
        Result::Done(_)
    ));
    let due = first + chrono::Duration::minutes(35);
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let row = r
        .statement_host()
        .read(Statement {
            sql: "SELECT due_at FROM notification_windows WHERE recipient_actor=?",
            args: vec![text(&actor)],
        })
        .await
        .unwrap();
    let early = r.due_for_session(&address, now(f)).await.unwrap();
    assert_eq!(early.messages.len(), 1);
    assert_eq!(early.messages[0].deliver_after, first);
    assert_eq!(
        row.rows,
        vec![vec![text(
            due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    assert_eq!(
        r.statement_host().alarm().map(|at| at.unix_millis()),
        Some(due.timestamp_millis())
    );
    drop(r);
    dispatch_to(f, due).await;
    advance_to(f, due + chrono::Duration::minutes(1));
    created(f.call(46006, post()).await);
    let next = due + chrono::Duration::minutes(15);
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    assert_eq!(
        r.statement_host().alarm().map(|at| at.unix_millis()),
        Some(next.timestamp_millis())
    );
    drop(r);
    dispatch_to(f, next).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let batches = r.due_for_session(&address, next).await.unwrap();
    assert_eq!(
        batches
            .messages
            .iter()
            .map(|message| message.deliver_after)
            .collect::<Vec<_>>(),
        vec![first, due, next]
    );
}

pub(super) async fn exercise_non_utc_and_terminal_rows(f: &Fixture) {
    let target = session(&f.caller);
    let result=request(f,&f.caller,46100,Op::SetAutomation{target:target.clone(),timing:serde_json::from_value(serde_json::json!({"kind":"cron","expression":"*/5 * * * *","timezone":"Asia/Kathmandu"})).unwrap(),text:body()}).await;
    let Result::Done(Answer::AutomationSet {
        automation_id,
        next_due_at: Some(first),
        ..
    }) = result
    else {
        panic!("nonUTC admission: {result:?}")
    };
    assert_eq!(first.timestamp().rem_euclid(300), 0);
    dispatch_to(f, first).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let batch = r.due_for_session(&target, first).await.unwrap();
    let fired:Vec<_>=batch.messages.iter().filter(|message|matches!(&message.body,MessageBody::Automation{automation_id:id,..}if id==&automation_id)).collect();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].deliver_after, first);
    let next = first + chrono::Duration::minutes(5);
    let row = r
        .statement_host()
        .read(Statement {
            sql: "SELECT next_due_at FROM automations WHERE automation_id=?",
            args: vec![text(&automation_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        row.rows,
        vec![vec![text(
            next.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    drop(r);
    // A distinct admitted request re-decodes the active non-UTC row before commit.
    assert!(matches!(
        request(
            f,
            &f.caller,
            46101,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M15
            }
        )
        .await,
        Result::Done(_)
    ));
    assert!(matches!(
        request(
            f,
            &f.caller,
            46102,
            Op::DisableAutomation {
                automation_id: automation_id.clone()
            }
        )
        .await,
        Result::Done(_)
    ));
    use central_storage::{AlarmUpdate, GuardedStatement, RowExpectation, StatementBatch};
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    // A retired rule is not reinterpreted against the current timezone database.
    assert!(matches!(r.statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"UPDATE automations SET timing_rule=? WHERE automation_id=?",args:vec![text(r#"{"kind":"cron","expression":"*/5 * * * *","timezone":"Retired/Timezone"}"#),text(&automation_id)]},expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
    assert!(matches!(
        request(
            f,
            &f.caller,
            46103,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M15
            }
        )
        .await,
        Result::Done(_)
    ));
    assert!(matches!(r.statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"UPDATE messages SET state='delivered',body='retired corrupt body' WHERE message_id=(SELECT message_id FROM messages WHERE message_kind='notice' AND state='ready' LIMIT 1)",args:vec![]},expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
    let area = f.area(46104).await;
    let thread = f.thread(46105, area).await;
    created(
        f.call(
            46106,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread,
                body: body(),
                addressees: vec![],
            },
        )
        .await,
    );
}

pub(super) async fn exercise_late_drain(f: &Fixture) {
    use central_storage::{AlarmUpdate, GuardedStatement, RowExpectation, StatementBatch};
    let mut caller = f.caller.clone();
    let mut address = session(&caller);
    address.session_id = "late-drain-recipient".to_owned().try_into().unwrap();
    caller.caller = m::Caller::Session(address.clone());
    request(
        f,
        &caller,
        46200,
        Op::SetNoticeCadence {
            cadence: m::NoticeCadence::M15,
        },
    )
    .await;
    let area = f.area(46201).await;
    let thread = f.thread(46202, area).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let actor=r.statement_host().read(Statement{sql:"SELECT actor_identity_id FROM actor_identities WHERE identity LIKE '%late-drain-recipient%'",args:vec![]}).await.unwrap().rows[0][0].clone();
    let central_storage::SqlValue::Text(id) = &actor else {
        panic!("recipient")
    };
    let actor_id = id.parse().unwrap();
    assert!(matches!(r.statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"CREATE TRIGGER reject_review_late_drain BEFORE UPDATE OF drained_through_position ON workspace_drains BEGIN SELECT RAISE(ABORT,'held drain'); END",args:vec![]},expect:RowExpectation::Any}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
    let response = f
        .call(
            46203,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread,
                body: body(),
                addressees: vec![m::Addressee::Actor {
                    actor_identity_id: actor_id,
                }],
            },
        )
        .await;
    assert_eq!(response.result, m::WorkspaceResult::OutcomeUnknown);
    let read = || Statement {
        sql: "SELECT due_at FROM notification_windows WHERE recipient_actor=?",
        args: vec![actor.clone()],
    };
    assert!(
        r.statement_host()
            .read(read())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    advance_to(
        f,
        now(f) + chrono::Duration::minutes(30) + chrono::Duration::seconds(20),
    );
    assert!(matches!(
        r.statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "DROP TRIGGER reject_review_late_drain",
                        args: vec![]
                    },
                    expect: RowExpectation::Any
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));
    r.drain_workspace(&f.workspace).await.unwrap();
    let due = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    assert_eq!(
        r.statement_host().read(read()).await.unwrap().rows,
        vec![vec![text(
            due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    drop(r);
    dispatch_to(f, due).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let batch = r.due_for_session(&address, due).await.unwrap();
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.messages[0].deliver_after, due);
}
