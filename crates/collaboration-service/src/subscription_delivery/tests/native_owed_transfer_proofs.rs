//! Completed-transfer input over a verified native prefix preserves exact follow debt.
#![deny(clippy::too_many_lines)]
use super::native_cancel_takeover_fixture::KeptProjectPrefix;
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;

#[tokio::test]
async fn completed_move_seed_delivers_the_inherited_owed_range_once() {
    prove_owed_transfer(false).await;
}
#[tokio::test]
async fn completed_takeover_seed_delivers_only_the_confirmed_owed_range_once() {
    prove_owed_transfer(true).await;
}

async fn prove_owed_transfer(discard_tail: bool) {
    let source = OwnerFixture::new().await;
    let first = source.post(&source.root, "first inherited owed post").await;
    let last = source
        .post(&source.root, "second inherited owed post")
        .await;
    let prefix = KeptProjectPrefix::capture(&source).await.unwrap();
    if discard_tail {
        source
            .post(&source.root, "discarded tail must not count as owed")
            .await;
    }
    let project = source.project.clone();
    let root = source.root.clone();
    let subject = source.discussion_subject.clone();
    let source_directory = source.into_directory();
    let mut successor = OwnerFixture::new().await;
    let installed = prefix
        .publish_successor(&successor, &project)
        .await
        .unwrap();
    assert_eq!(installed.position.epoch.value(), 1);
    successor.project = project;
    successor.root = root;
    successor.discussion_subject = subject;
    drop(source_directory);
    let runtime = successor.runtime().await;
    let (application, native, publisher) = follow_application(&successor, &runtime).await;
    publisher
        .queue_project(successor.project.clone())
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    let frozen = frozen_follow(&successor).await;
    assert_eq!(frozen.len(), 1);
    let item = frozen.first().unwrap();
    assert_eq!(
        request.payload.push_id.as_str(),
        item.id().as_uuid().to_string()
    );
    let BoardDeliveryBody::Notification(body) = item.body() else {
        panic!("frozen follow notification");
    };
    let NotificationSource::FollowBatch {
        generation, roots, ..
    } = &body.source
    else {
        panic!("frozen follow source");
    };
    // Fixture join creates generation one; its explicit zero-window policy is
    // the first policy change, so inherited debt belongs to generation two.
    assert_eq!(generation.get(), 2);
    assert_eq!(roots.len(), 1);
    let owed = roots.first().unwrap();
    assert_eq!(owed.from(), first);
    assert_eq!(owed.through(), last);
    assert_eq!(owed.post_count(), 2);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    publisher
        .queue_project(successor.project.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        publisher.wait_project_idle_for_test(&successor.project),
    )
    .await
    .unwrap();
    assert_eq!(frozen_follow(&successor).await.len(), 1);
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    publisher.shutdown().await;
    drop(publisher);
    drop(application);
    runtime.close().await;
    successor.close_native(native).await;
}
