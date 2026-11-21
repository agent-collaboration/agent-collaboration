use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use fleet_contracts::mail_link::WelcomeDeliveryPage;

#[hegel::test(test_cases = 15)]
fn an_expired_notice_stays_cancelled_through_a_replayed_drain(case: hegel::TestCase) {
    use hegel::generators as gs;
    let after_deadline = case.draw(gs::integers::<i64>().min_value(0).max_value(30_000));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(expiry_and_replayed_drain(after_deadline));
}

async fn expiry_and_replayed_drain(after_deadline: i64) {
    let fixture = Fixture::new().await;
    let area = fixture.area(7320).await;
    let thread = fixture.thread(7321, area).await;
    let (recipient, actor) = joined_observer(&fixture, &thread).await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let posted = fixture.call(7322, post(&thread, vec![])).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { position, .. }) = posted.result
    else {
        panic!("fixture post did not create activity");
    };
    let pending_window = || Statement {
        sql: "SELECT due_at FROM mail_notification_windows WHERE recipient_actor=?",
        args: vec![text(&actor)],
    };
    let first_window = runtime
        .runtime()
        .statement_host()
        .read(pending_window())
        .await
        .unwrap();
    let window_due: m::Timestamp = row_text(&first_window.rows[0][0]).parse().unwrap();
    fixture.set_utc(window_due).await;
    runtime.after_event().await.unwrap();

    let notices = || Statement {
        sql: "SELECT message_id,delivery_window_ends_at,state,failure_reason FROM mail_messages WHERE message_kind='notice' AND recipient_actor=? AND source_thread_id=?",
        args: vec![text(&actor), text(&thread)],
    };
    let original = runtime
        .runtime()
        .statement_host()
        .read(notices())
        .await
        .unwrap()
        .rows;
    let [notice] = original.as_slice() else {
        panic!("expected one original notice: {original:?}")
    };
    let message_id: m::MessageId = row_text(&notice[0]).parse().unwrap();
    let expiry: m::Timestamp = row_text(&notice[1]).parse().unwrap();
    assert_eq!(expiry, window_due + chrono::Duration::hours(24));
    assert_eq!(notice[2], text("queued"));

    fixture
        .set_utc(expiry - chrono::Duration::milliseconds(1))
        .await;
    let before = fixture
        .fresh_caller(&recipient)
        .await
        .connection()
        .attachment();
    let WelcomeDeliveryPage::Ready(eligible) =
        runtime.windowed_welcome_page(&before, None).await.unwrap()
    else {
        panic!("recipient authority lapsed")
    };
    assert!(
        eligible
            .iter()
            .any(|row| row.message.message_id == message_id)
    );
    fixture
        .set_utc(expiry + chrono::Duration::milliseconds(after_deadline))
        .await;
    // The expired window itself excludes the row, even before running the expiry batch.
    let WelcomeDeliveryPage::Ready(eligible) =
        runtime.windowed_welcome_page(&before, None).await.unwrap()
    else {
        panic!("recipient authority lapsed")
    };
    assert!(
        eligible
            .iter()
            .all(|row| row.message.message_id != message_id)
    );
    runtime.after_event().await.unwrap();
    let expired = runtime
        .runtime()
        .statement_host()
        .read(notices())
        .await
        .unwrap()
        .rows;
    assert_eq!(
        expired,
        vec![vec![
            notice[0].clone(),
            notice[1].clone(),
            text("cancelled"),
            text("expired24Hours")
        ]]
    );

    // Rewind only the last drain's cursor and run the real drain/window writers again.
    let rewind = runtime.runtime().statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE mail_workspace_drains SET drained_through_position=? WHERE workspace_id=?",
                args: vec![SqlValue::Integer(i64::try_from(position.get() - 1).unwrap().try_into().unwrap()), text(&fixture.workspace)],
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
    let replay_window = runtime
        .runtime()
        .statement_host()
        .read(pending_window())
        .await
        .unwrap();
    let replay_due = row_text(&replay_window.rows[0][0]).parse().unwrap();
    fixture.set_utc(replay_due).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        runtime
            .runtime()
            .statement_host()
            .read(notices())
            .await
            .unwrap()
            .rows,
        expired
    );
    assert!(
        runtime
            .runtime()
            .statement_host()
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
        panic!("expected stored text")
    };
    value
}
