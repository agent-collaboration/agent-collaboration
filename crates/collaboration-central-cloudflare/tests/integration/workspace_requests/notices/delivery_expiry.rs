use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};

#[tokio::test]
async fn an_expired_notice_stays_cancelled_through_a_replayed_drain() {
    let fixture = Fixture::new().await;
    let area = fixture.area(7320).await;
    let thread = fixture.thread(7321, area).await;
    let (_, actor) = joined_observer(&fixture, &thread).await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let posted = fixture.call(7322, post(&thread, vec![])).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { position, .. }) = posted.result
    else {
        panic!("fixture post did not create activity");
    };
    let pending_window = || Statement {
        sql: "SELECT due_at FROM mail_notification_windows WHERE recipient_actor=?",
        args: vec![text(&actor)],
    };
    let first_window = runtime.read(pending_window()).await.unwrap();
    let window_due: m::Timestamp = row_text(&first_window.rows[0][0]).parse().unwrap();
    fixture
        .clock
        .0
        .store(window_due.timestamp_millis(), Ordering::SeqCst);
    runtime.after_event().await.unwrap();

    let notices = || Statement {
        sql: "SELECT message_id,delivery_window_ends_at,state,failure_reason FROM mail_messages WHERE message_kind='notice' AND recipient_actor=? AND source_thread_id=?",
        args: vec![text(&actor), text(&thread)],
    };
    let original = runtime.read(notices()).await.unwrap().rows;
    let [notice] = original.as_slice() else {
        panic!("expected one original notice: {original:?}");
    };
    let expiry: m::Timestamp = row_text(&notice[1]).parse().unwrap();
    assert_eq!(expiry, window_due + chrono::Duration::hours(24));
    assert_eq!(notice[2], text("queued"));
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(expiry.timestamp_millis())
    );

    fixture
        .clock
        .0
        .store(expiry.timestamp_millis() - 1, Ordering::SeqCst);
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_none());
    assert_eq!(runtime.read(notices()).await.unwrap().rows, original);
    fixture
        .clock
        .0
        .store(expiry.timestamp_millis(), Ordering::SeqCst);
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_some());
    let expired = runtime.read(notices()).await.unwrap().rows;
    assert_eq!(
        expired,
        vec![vec![
            notice[0].clone(),
            notice[1].clone(),
            text("cancelled"),
            text("expired24Hours"),
        ]]
    );

    let rewind = runtime.run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE mail_workspace_drains SET drained_through_position=? WHERE workspace_id=?",
                args: vec![
                    SqlValue::Integer(i64::try_from(position.get()-1).unwrap().try_into().unwrap()),
                    text(&fixture.workspace),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::Unchanged,
    }).await.unwrap();
    assert!(matches!(rewind, BatchOutcome::Applied { .. }));
    assert_eq!(
        runtime
            .drain_workspace(&fixture.workspace)
            .await
            .unwrap()
            .window_items_added,
        1
    );
    let replay_window = runtime.read(pending_window()).await.unwrap();
    let replay_due: m::Timestamp = row_text(&replay_window.rows[0][0]).parse().unwrap();
    fixture
        .clock
        .0
        .store(replay_due.timestamp_millis(), Ordering::SeqCst);
    runtime.after_event().await.unwrap();
    assert_eq!(runtime.read(notices()).await.unwrap().rows, expired);
    assert!(
        runtime
            .read(Statement {
                sql: "SELECT message_id FROM mail_messages WHERE message_kind='deliveryOutcome'",
                args: vec![],
            })
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    fixture.host.close().await;
}

fn row_text(value: &SqlValue) -> &str {
    let SqlValue::Text(value) = value else {
        panic!("expected stored text");
    };
    value
}
