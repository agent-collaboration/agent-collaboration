use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, ConstraintKind, CorruptReason, GuardedStatement, RowExpectation,
    StatementBatch, StoreFailure,
};

async fn execute(f: &Fixture, sql: &'static str, args: Vec<SqlValue>) {
    let runtime = f.host.central(&f.caller.organization_id).await;
    let result = runtime
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
    assert!(matches!(result, BatchOutcome::Applied { .. }));
}

#[tokio::test]
async fn invalid_open_window_is_rejected_before_alarm_changes_queue_or_cursor() {
    for (sql, expected_column) in [
        (
            "UPDATE notification_windows SET due_at='not-a-timestamp' WHERE recipient_actor=?",
            "notification_windows.due_at",
        ),
        (
            "UPDATE notification_windows SET due_at='2020-01-01T00:00:00Z' WHERE recipient_actor=?",
            "notification_windows.due_at",
        ),
        (
            "UPDATE notification_windows SET recipient_actor='invalid-uuid' WHERE recipient_actor=?",
            "notification_windows.recipient_actor",
        ),
    ] {
        let f = Fixture::new().await;
        let area = f.area(7500).await;
        let thread = f.thread(7501, area).await;
        let (_, actor) = joined_observer(&f, &thread).await;
        created(f.call(7502, post(&thread, vec![])).await);
        let runtime = f.host.central(&f.caller.organization_id).await;
        let read = || Statement {
            sql: "SELECT (SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?),(SELECT COUNT(*) FROM notification_windows WHERE window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)),(SELECT COUNT(*) FROM notification_window_items),(SELECT COUNT(*) FROM messages WHERE message_kind='notice')",
            args: vec![text(&f.workspace), text(&thread)],
        };
        let before = runtime.statement_host().read(read()).await.unwrap();
        assert_eq!(before.rows[0][1], SqlValue::Integer(2.try_into().unwrap()));
        assert_eq!(before.rows[0][2], SqlValue::Integer(2.try_into().unwrap()));
        assert_eq!(before.rows[0][3], SqlValue::Integer(0.try_into().unwrap()));
        let prior_alarm: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
        assert!(prior_alarm.is_some());
        execute(&f, sql, vec![text(&actor)]).await;
        let error = runtime.on_alarm().await.unwrap_err();
        assert!(
            matches!(error, StoreFailure::Corrupt { reason: CorruptReason::BadValue { column } } if column == expected_column)
        );
        assert_eq!(
            runtime.statement_host().read(read()).await.unwrap().rows,
            before.rows
        );
        let receipt: Option<super::super::workerd_host::AlarmReceipt> =
            runtime.control("raw-alarm-status", &()).await.unwrap();
        let receipt = receipt.unwrap();
        assert!(matches!(
            receipt.result,
            super::super::workerd_host::AlarmReceiptResult::Error { .. }
        ));
        assert_eq!(receipt.alarm_before, receipt.alarm_after);
        let recipient = runtime.statement_host().read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND message_kind='notice'", args: vec![text(actor)] }).await.unwrap();
        assert_eq!(
            recipient.rows,
            vec![vec![SqlValue::Integer(0.try_into().unwrap())]]
        );
        f.host.close().await;
    }
}

#[tokio::test]
async fn later_drain_statement_failure_rolls_back_notices_windows_and_cursor_then_retries_once() {
    let f = Fixture::new().await;
    let area = f.area(7510).await;
    let thread = f.thread(7511, area).await;
    let (_, observer) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    // Addressing the author adds a second recipient while the observer remains in the same batch window.
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let author: m::ActorIdentityId = actors
        .rows
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (SqlValue::Text(id), SqlValue::Text(identity)) if identity.contains("test-session") => {
                Some(id.parse().unwrap())
            }
            _ => None,
        })
        .unwrap();
    let prior = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position,(SELECT COUNT(*) FROM messages WHERE source_thread_id=? AND message_kind='notice'),(SELECT COUNT(*) FROM notification_windows WHERE window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)),(SELECT COUNT(*) FROM notification_window_items) FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&thread), text(&thread), text(&f.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(
        &prior.rows[0][1..],
        &[
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]
    );
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        let operation = post(
            &thread,
            vec![m::Addressee::Actor {
                actor_identity_id: author.clone(),
            }],
        );
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(7512)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    pending.abort();
    let _ = pending.await;
    execute(&f, "CREATE TRIGGER reject_drain_cursor BEFORE UPDATE ON workspace_drains WHEN NEW.drained_through_position != OLD.drained_through_position BEGIN SELECT RAISE(ABORT,'reject drain cursor'); END", vec![]).await;
    assert!(matches!(
        runtime.drain_workspace(&f.workspace).await,
        Err(StoreFailure::Unavailable)
    ));
    let after_failure = runtime.statement_host().read(Statement { sql: "SELECT drained_through_position,drain_due_at,(SELECT COUNT(*) FROM messages WHERE source_thread_id=? AND message_kind='notice'),(SELECT COUNT(*) FROM notification_windows WHERE window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)),(SELECT COUNT(*) FROM notification_window_items) FROM workspace_drains WHERE workspace_id=?", args: vec![text(&thread), text(&thread), text(&f.workspace)] }).await.unwrap();
    assert_eq!(after_failure.rows[0][0], prior.rows[0][0]);
    assert!(matches!(&after_failure.rows[0][1], SqlValue::Text(_)));
    assert_eq!(&after_failure.rows[0][2..], &prior.rows[0][1..]);
    execute(&f, "DROP TRIGGER reject_drain_cursor", vec![]).await;
    let result = runtime.drain_workspace(&f.workspace).await.unwrap();
    assert_eq!(result.notices_queued, 0);
    assert_eq!(result.window_items_added, 2);
    let again = runtime.drain_workspace(&f.workspace).await.unwrap();
    assert_eq!(again.notices_queued, 0);
    assert_eq!(again.window_items_added, 0);
    let after_retry = runtime.statement_host().read(Statement { sql: "SELECT drained_through_position,drain_due_at,(SELECT COUNT(*) FROM messages WHERE source_thread_id=? AND recipient_actor=? AND message_kind='notice'),(SELECT COUNT(*) FROM notification_windows WHERE window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?) AND recipient_actor=?),(SELECT COUNT(*) FROM notification_window_items) FROM workspace_drains WHERE workspace_id=?", args: vec![text(&thread), text(&author), text(&thread), text(&observer), text(&f.workspace)] }).await.unwrap();
    assert_ne!(after_retry.rows[0][0], prior.rows[0][0]);
    assert_eq!(after_retry.rows[0][1], SqlValue::Null);
    assert_eq!(
        &after_retry.rows[0][2..],
        &[
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(3.try_into().unwrap())
        ]
    );
    let duplicate = runtime.statement_host().run_batch(StatementBatch { statements: vec![GuardedStatement { statement: Statement {
        sql: "INSERT INTO notification_window_items(window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at) SELECT ?,window_id,workspace_id,thread_id,change_id,created_at,updated_at FROM notification_window_items", args: vec![text(id::<m::WorkspaceId>(7513))],
    }, expect: RowExpectation::Exactly(1) }], alarm: AlarmUpdate::Unchanged }).await;
    assert!(matches!(
        duplicate,
        Ok(BatchOutcome::ConstraintViolated {
            index: 0,
            kind: ConstraintKind::Unique
        })
    ));
    f.host.close().await;
}

#[tokio::test]
async fn corrupt_existing_notice_body_stops_drain_before_cursor_or_new_effects() {
    let f = Fixture::new().await;
    let area = f.area(7520).await;
    let thread = f.thread(7521, area).await;
    let (_, recipient) = joined_observer(&f, &thread).await;
    created(
        f.call(
            7522,
            post(
                &thread,
                vec![m::Addressee::Actor {
                    actor_identity_id: recipient.clone(),
                }],
            ),
        )
        .await,
    );
    let runtime = f.host.central(&f.caller.organization_id).await;
    f.clock.0.fetch_add(900_000, Ordering::SeqCst);
    runtime.on_alarm().await.unwrap();
    let before = runtime.statement_host().read(Statement { sql: "SELECT drained_through_position,(SELECT COUNT(*) FROM messages WHERE source_thread_id=? AND message_kind='notice') FROM workspace_drains WHERE workspace_id=?", args: vec![text(&thread), text(&f.workspace)] }).await.unwrap();
    assert_eq!(before.rows[0][1], SqlValue::Integer(2.try_into().unwrap()));
    execute(
        &f,
        "UPDATE messages SET body='not-json' WHERE source_thread_id=? AND message_kind='notice'",
        vec![text(&thread)],
    )
    .await;
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        let operation = post(
            &thread,
            vec![m::Addressee::Actor {
                actor_identity_id: recipient.clone(),
            }],
        );
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(7523)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    pending.abort();
    let _ = pending.await;
    let error = runtime.drain_workspace(&f.workspace).await.unwrap_err();
    assert!(
        matches!(error, StoreFailure::Corrupt { reason: CorruptReason::BadValue { column } } if column == "messages.body")
    );
    let after = runtime.statement_host().read(Statement { sql: "SELECT drained_through_position,(SELECT COUNT(*) FROM messages WHERE source_thread_id=? AND message_kind='notice'),drain_due_at FROM workspace_drains WHERE workspace_id=?", args: vec![text(&thread), text(&f.workspace)] }).await.unwrap();
    assert_eq!(&after.rows[0][..2], &before.rows[0][..]);
    assert!(matches!(&after.rows[0][2], SqlValue::Text(_)));
    f.host.close().await;
}
