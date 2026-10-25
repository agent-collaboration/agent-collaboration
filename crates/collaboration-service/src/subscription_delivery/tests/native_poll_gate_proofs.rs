//! The same registered wait survives a temporary checker failure; absence never authorizes it.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use crate::{SubscriptionWaitFilter, SubscriptionWaitResult};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem};
use message_board::{SubscriptionMode, WhenIdle};
use message_board_storage::MailHandoffItem;
use sqlx::Connection;

#[tokio::test]
async fn poll_without_its_publisher_stays_held_until_the_same_wait_is_reconciled() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(&fixture.root, "check before handing over")
        .await;
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    let item = frozen[0].id();
    let push: collaboration_protocol::PushId = item.as_uuid().to_string().try_into().unwrap();
    runtime
        .service
        .attach_follow_admission(std::sync::Weak::new(), std::sync::Weak::new());
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let mut waiting = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
    });
    let checked = tokio::time::timeout(Duration::from_secs(6), runtime.observe(|event| {
        matches!(event, OwnerObservation::PollPublisherUnavailable(observed) if *observed==item)
    })).await;
    runtime.owner_barrier(&fixture.reader).await;
    let handed_over_without_publisher = waiting.is_finished();
    let early_push = fixture.interactions.get_push_record(&push).await.unwrap();
    runtime.service.attach_follow_admission(
        Arc::downgrade(&fixture.admission),
        Arc::downgrade(&publisher),
    );
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(6), &mut waiting).await;
    if response.is_err() {
        waiting.abort();
        let _ = waiting.await;
    }
    if response.is_ok() {
        fixture
            .await_push_state(&push, PushDeliveryState::Delivered)
            .await;
    }
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        checked.is_ok(),
        "the SDK Poll callback did not inspect its missing publisher"
    );
    assert!(
        !handed_over_without_publisher && early_push.is_none(),
        "Poll bypassed the missing cancellation checker"
    );
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = response
        .expect("the same registered wait did not recover")
        .unwrap()
        .unwrap()
        .unwrap()
    else {
        panic!("session notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
}

#[tokio::test]
async fn a_temporary_poll_check_failure_reconsiders_the_same_item_on_an_existing_rescan() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(&fixture.root, "one recoverable checker failure")
        .await;
    // A command barrier does not order an independently delivered activity hint.
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    let item = &frozen[0];
    let id = item.id();
    let push: collaboration_protocol::PushId = id.as_uuid().to_string().try_into().unwrap();
    let database = fixture.registry.database(&fixture.project).await.unwrap();
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(database.path()),
    )
    .await
    .unwrap();
    // Corrupt only the project owner's frozen content, keeping a valid envelope
    // and unchanged root coverage. The real cancellation checker must detect its
    // disagreement with the SDK's immutable original digest. No SDK SQL is used.
    let original: String = sqlx::query_scalar("SELECT item FROM mail_handoffs WHERE item_id=?")
        .bind(push.as_str())
        .fetch_one(&mut observer)
        .await
        .unwrap();
    let mut body = item.body().clone();
    let BoardDeliveryBody::Notification(notification) = &mut body else {
        panic!("notification");
    };
    notification.excerpt = "temporary project read disagreement"
        .to_owned()
        .try_into()
        .unwrap();
    let divergent = DeliveryItem::<BoardDeliveryTypes>::new(
        id,
        item.sender().clone(),
        item.recipient().clone(),
        item.valid_until(),
        body,
    )
    .unwrap();
    let changed = serde_json::to_string(&MailHandoffItem::Delivery(Box::new(
        DeliveryEnvelope::Item(divergent),
    )))
    .unwrap();
    sqlx::query("UPDATE mail_handoffs SET item=? WHERE item_id=?")
        .bind(changed)
        .bind(push.as_str())
        .execute(&mut observer)
        .await
        .unwrap();
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let mut waiting = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
    });
    let deferred=tokio::time::timeout(Duration::from_secs(6), runtime.observe(|event| {
        matches!(event,OwnerObservation::PollCancellationDeferred(observed) if *observed==id)
    })).await;
    runtime.owner_barrier(&fixture.reader).await;
    let early_push = fixture.interactions.get_push_record(&push).await.unwrap();
    let handed_over_while_invalid = waiting.is_finished();
    sqlx::query("UPDATE mail_handoffs SET item=? WHERE item_id=?")
        .bind(original)
        .bind(push.as_str())
        .execute(&mut observer)
        .await
        .unwrap();
    observer.close().await.unwrap();
    drop(database);
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(6), &mut waiting).await;
    if response.is_err() {
        waiting.abort();
        let _ = waiting.await;
    }
    if response.is_ok() {
        fixture
            .await_push_state(&push, PushDeliveryState::Delivered)
            .await;
    }
    let native_called = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        deferred.is_ok(),
        "the real Poll cancellation checker never observed the temporary mismatch"
    );
    assert!(!handed_over_while_invalid && early_push.is_none());
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = response
        .expect("a temporary cancellation failure excluded the item for the life of this wait")
        .unwrap()
        .unwrap()
        .unwrap()
    else {
        panic!("session notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
    assert!(!native_called);
}
