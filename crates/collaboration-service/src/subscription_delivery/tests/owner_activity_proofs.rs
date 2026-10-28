//! A real owner's idle barrier does not acknowledge an activity selection.
use super::super::subscription_service::OwnerObservation;
use super::super::{SubscriptionWaitFilter, SubscriptionWaitResult};
use super::owner_fixture::*;
use futures_util::poll;
use sqlx::Connection;

#[tokio::test]
async fn activity_waiter_does_not_complete_at_a_barrier_before_the_post_is_selected() {
    Box::pin(tokio::time::timeout(
        std::time::Duration::from_secs(30),
        activity_selection_journey(),
    ))
    .await
    .expect("bounded human Poll selection journey");
}

async fn activity_selection_journey() {
    let fixture = Box::pin(OwnerFixture::with_reader(human("activity-reader"))).await;
    *fixture.presence.0.lock().unwrap() = collaboration_contracts::TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    runtime.owner_barrier(&fixture.reader).await;
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let waiting = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
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
    let mut blocker = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
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
    let SubscriptionWaitResult::Ranges { batch } = waiting.await.unwrap().unwrap().unwrap() else {
        panic!("human Poll returns literal ranges without a session notice");
    };
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].project_id, fixture.project);
    assert_eq!(batch.roots[0].thread_id, fixture.root);
    assert_eq!(batch.roots[0].from, posted);
    assert_eq!(batch.roots[0].through, posted);
    assert_eq!(batch.roots[0].post_count, 1);
    assert!(runtime.requests.lock().await.try_recv().is_err());
    blocker.close().await.unwrap();
    runtime.close().await;
}
