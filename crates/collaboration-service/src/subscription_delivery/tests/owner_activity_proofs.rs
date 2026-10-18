//! A real owner's idle barrier does not acknowledge an activity selection.
use super::super::SubscriptionClock;
use super::super::subscription_service::OwnerObservation;
use super::owner_fixture::*;
use futures_util::poll;
use sqlx::Connection;

#[tokio::test]
async fn activity_waiter_does_not_complete_at_a_barrier_before_the_post_is_selected() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = crate::TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    runtime.owner_barrier(&fixture.reader).await;
    let mut selected = Box::pin(runtime.wait_for_activity(&fixture.reader, |event| {
        matches!(event, OwnerObservation::Selected)
    }));
    assert!(poll!(selected.as_mut()).is_pending());
    // This separate fence acknowledges every earlier owner command. A bare
    // barrier helper can finish here even though no activity was selected.
    runtime
        .service
        .synchronize_reader(fixture.reader.clone())
        .await;
    let mut blocker = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("automation.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut blocker)
        .await
        .unwrap();
    let posted = fixture
        .post(&fixture.root, "Actual activity awaiting its progress write")
        .await;
    // The real project commit precedes this poll. The reserved SQLite writer
    // prevents the owner's progress/selection write; it cannot emit Selected.
    let completed_before_selection = poll!(selected.as_mut()).is_ready();
    sqlx::query("COMMIT").execute(&mut blocker).await.unwrap();
    if completed_before_selection {
        drop(selected);
        runtime.close().await;
        panic!("activity helper returned at a bare barrier before the posted record was selected");
    }
    selected.await;
    let subscription = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(
            &fixture.reader,
            &message_board::SubscriptionScope::thread(fixture.root.clone()),
            fixture.clock.now(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(subscription.roots().len(), 1);
    assert_eq!(subscription.roots()[0].pending_count(), 1);
    assert!(posted.position > message_board::FeedPosition::try_from(0_u64).unwrap());
    assert!(runtime.requests.lock().await.try_recv().is_err());
    blocker.close().await.unwrap();
    runtime.close().await;
}
