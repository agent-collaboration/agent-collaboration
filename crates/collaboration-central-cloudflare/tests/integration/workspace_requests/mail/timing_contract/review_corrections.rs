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
    let r = f.host.central(&f.caller.organization_id).await;
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
    let first =
        chrono::DateTime::from_timestamp((start.timestamp().div_euclid(3300) + 1) * 3300, 0)
            .unwrap();
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
    let due = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(900) + 1) * 900, 0)
        .unwrap();
    let r = f.host.central(&f.caller.organization_id).await;
    let row = r
        .statement_host()
        .read(Statement {
            sql: "SELECT due_at FROM notification_windows WHERE recipient_actor=?",
            args: vec![text(&actor)],
        })
        .await
        .unwrap();
    let early = r.queued_for_session(&address).await.unwrap();
    assert_eq!(early.messages.len(), 1);
    assert_eq!(early.messages[0].created_at, first);
    assert_eq!(
        row.rows,
        vec![vec![text(
            due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(due.timestamp_millis())
    );
    drop(r);
    dispatch_to(f, due).await;
    advance_to(f, due + chrono::Duration::minutes(1));
    created(f.call(46006, post()).await);
    let next = due + chrono::Duration::minutes(15);
    let r = f.host.central(&f.caller.organization_id).await;
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(next.timestamp_millis())
    );
    drop(r);
    dispatch_to(f, next).await;
    let r = f.host.central(&f.caller.organization_id).await;
    let batches = r.queued_for_session(&address).await.unwrap();
    assert_eq!(
        batches
            .messages
            .iter()
            .map(|message| message.created_at)
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
    let r = f.host.central(&f.caller.organization_id).await;
    let batch = r.queued_for_session(&target).await.unwrap();
    let fired:Vec<_>=batch.messages.iter().filter(|message|matches!(&message.body,MessageBody::Automation{automation_id:id,..}if id==&automation_id)).collect();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].created_at, first);
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
    let r = f.host.central(&f.caller.organization_id).await;
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
    assert!(matches!(r.statement_host().run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"UPDATE messages SET state='delivered',body='retired corrupt body' WHERE message_id=(SELECT message_id FROM messages WHERE message_kind='notice' AND state='queued' LIMIT 1)",args:vec![]},expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged}).await.unwrap(),central_storage::BatchOutcome::Applied{..}));
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
    let r = f.host.central(&f.caller.organization_id).await;
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
    let due = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(900) + 1) * 900, 0)
        .unwrap();
    assert_eq!(
        r.statement_host().read(read()).await.unwrap().rows,
        vec![vec![text(
            due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    drop(r);
    dispatch_to(f, due).await;
    let r = f.host.central(&f.caller.organization_id).await;
    let batch = r.queued_for_session(&address).await.unwrap();
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.messages[0].created_at, due);
}

pub(super) async fn exercise_cadence_change_after_clock_rollback(f: &mut Fixture) {
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
    };
    let activity: m::Timestamp = "2030-01-09T12:26:00Z".parse().unwrap();
    advance_to(f, activity);
    let mut caller = f.caller.clone();
    let mut address = session(&caller);
    address.session_id = "rollback-cadence".to_owned().try_into().unwrap();
    caller.caller = m::Caller::Session(address.clone());
    assert_eq!(
        request(
            f,
            &caller,
            46300,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M55
            }
        )
        .await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M55
        })
    );
    let area = f.area(46301).await;
    let thread = f.thread(46302, area).await;
    let r = f.host.central(&f.caller.organization_id).await;
    let actors = r.statement_host().read(Statement {sql:"SELECT actor_identity_id FROM actor_identities WHERE json_extract(identity,'$.session.sessionId')='rollback-cadence'",args:vec![]}).await.unwrap();
    assert_eq!(actors.rows.len(), 1);
    let actor = actors.rows[0][0].clone();
    let central_storage::SqlValue::Text(actor_id) = &actor else {
        panic!("actor")
    };
    let post = created(
        f.call(
            46303,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: body(),
                addressees: vec![m::Addressee::Actor {
                    actor_identity_id: actor_id.parse().unwrap(),
                }],
            },
        )
        .await,
    );
    // Settle pre-existing due work so the retained alarm is stable for the late-write cut.
    r.on_alarm().await.unwrap();
    let snapshot = || Statement {
        sql: "SELECT a.notice_cadence,w.window_id,w.first_activity_at,w.due_at,(SELECT json_group_array(json_array(i.window_item_id,i.workspace_id,i.thread_id,i.change_id,i.created_at,i.updated_at)) FROM notification_window_items i WHERE i.window_id=w.window_id),(SELECT COUNT(*) FROM request_log WHERE request_id=?),a.created_at,a.updated_at,w.created_at,w.updated_at FROM actor_identities a JOIN notification_windows w ON w.recipient_actor=a.actor_identity_id WHERE a.actor_identity_id=?",
        args: vec![text(id::<m::RequestId>(46304)), actor.clone()],
    };
    let ledger = || Statement {
        sql: "SELECT request_id,caller,content_digest,answer,created_at,updated_at FROM request_log ORDER BY request_id",
        args: vec![],
    };
    let before_ledger = r.statement_host().read(ledger()).await.unwrap().rows;
    let before = r.statement_host().read(snapshot()).await.unwrap().rows;
    // The schema permits one window for this actor, aggregating all its sources.
    assert_eq!(before.len(), 1);
    assert_eq!(before[0][0], text("m55"));
    assert_eq!(before[0][2], text("2030-01-09T12:26:00.000Z"));
    assert_eq!(before[0][3], text("2030-01-09T12:50:00.000Z"));
    let original_items: serde_json::Value = match &before[0][4] {
        central_storage::SqlValue::Text(items) => serde_json::from_str(items).unwrap(),
        _ => panic!("items"),
    };
    assert_eq!(original_items.as_array().unwrap().len(), 1);
    let rewind: m::Timestamp = "2030-01-09T11:30:00Z".parse().unwrap();
    f.clock.0.store(
        rewind.timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
    let prior_alarm = r
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    let write = |sql| StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement { sql, args: vec![] },
            expect: RowExpectation::Any,
        }],
        alarm: AlarmUpdate::Unchanged,
    };
    assert!(matches!(r.statement_host().run_batch(write("CREATE TRIGGER reject_cadence_deadline BEFORE UPDATE OF due_at ON notification_windows BEGIN SELECT RAISE(IGNORE); END")).await.unwrap(),BatchOutcome::Applied{..}));
    let change = Op::SetNoticeCadence {
        cadence: m::NoticeCadence::M15,
    };
    assert_eq!(
        request(f, &caller, 46304, change.clone()).await,
        Result::Conflict
    );
    assert_eq!(
        r.statement_host().read(snapshot()).await.unwrap().rows,
        before,
        "cadence, every open window, source items and ledger must roll back together"
    );
    assert_eq!(
        r.statement_host().read(ledger()).await.unwrap().rows,
        before_ledger
    );
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        prior_alarm
    );
    assert!(matches!(
        r.statement_host()
            .run_batch(write("DROP TRIGGER reject_cadence_deadline"))
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(
        request(f, &caller, 46304, change.clone()).await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M15
        })
    );
    let mut expected = before;
    expected[0][0] = text("m15");
    expected[0][3] = text("2030-01-09T11:45:00.000Z");
    expected[0][5] = central_storage::SqlValue::Integer(1.try_into().unwrap());
    expected[0][7] = text("2030-01-09T11:30:00.000Z");
    expected[0][9] = text("2030-01-09T11:30:00.000Z");
    assert_eq!(
        r.statement_host().read(snapshot()).await.unwrap().rows,
        expected
    );
    assert_eq!(
        request(f, &caller, 46304, change).await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M15
        })
    );
    assert_eq!(
        r.statement_host().read(snapshot()).await.unwrap().rows,
        expected
    );
    assert!(
        r.queued_for_session(&address)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    drop(r);
    f.reopen().await;
    let r = f.host.central(&f.caller.organization_id).await;
    assert_eq!(
        r.statement_host().read(snapshot()).await.unwrap().rows,
        expected
    );
    let due: m::Timestamp = "2030-01-09T11:45:00Z".parse().unwrap();
    assert!(due > rewind && due < activity);
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(due.timestamp_millis())
    );
    drop(r);
    dispatch_to(f, due).await;
    let r = f.host.central(&f.caller.organization_id).await;
    let queue = r.queued_for_session(&address).await.unwrap();
    assert_eq!(queue.messages.len(), 1);
    assert_eq!(queue.messages[0].created_at, due);
    let MessageBody::Notice { items } = &queue.messages[0].body else {
        panic!("notice")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item, post);
    assert_eq!(items[0].workspace_id, f.workspace);
    assert_eq!(items[0].thread_id, Some(thread));
    assert_eq!(
        items[0].change_id.to_string(),
        original_items[0][3].as_str().unwrap()
    );
    assert_eq!(items[0].event, m::ChangeEvent::Posted);
    assert!(
        r.statement_host()
            .read(snapshot())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    r.on_alarm().await.unwrap();
    assert_eq!(r.queued_for_session(&address).await.unwrap(), queue);
}
