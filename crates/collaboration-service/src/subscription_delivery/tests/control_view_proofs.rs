//! The public subscriptions view must expose the owner's durable hold and retry facts,
//! and a reconciliation receipt must cover the owner's window scan.
use router_clock::RuntimeClock;

use super::owner_fixture::*;
use crate::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_protocol::ThreadSubscriptionsRequest;
use sqlx::Connection;

#[path = "subscription_observation_proofs.rs"]
mod native_observations;

#[tokio::test]
async fn reconcile_receipt_waits_for_the_owner_to_finish_its_native_window_scan() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Poll,
            message_board::WhenIdle::Hold,
            0,
            900,
        )
        .await;
    let runtime = fixture.runtime().await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
        .await;
    // The real window-store mutex is the owner boundary, not a delay estimate.
    let guard = fixture.store.lock().await;
    let mut response = runtime
        .service
        .request_reconcile_receipt(fixture.reader.clone())
        .await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::ReconcileRequested))
        .await;
    assert!(
        matches!(
            response.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "reconciliation was acknowledged before the native window scan could run"
    );
    fixture
        .post(&fixture.root, "Receipt must cover this pending activity")
        .await;
    drop(guard);
    let outcome = (response)
        .await
        .expect("owner retained the reconcile reply");
    assert_eq!(outcome.as_ref().map(|_| ()), Ok(()));
    assert_eq!(outcome.as_ref().unwrap().records.len(), 1);
    assert_eq!(
        outcome.as_ref().unwrap().records[0]
            .record
            .roots()
            .first()
            .unwrap()
            .pending_count(),
        1
    );
    let records = fixture
        .store
        .lock()
        .await
        .list_reader_subscriptions(&fixture.reader, fixture.clock.utc_now())
        .await
        .unwrap();
    assert_eq!(records.records.len(), 1);
    assert_eq!(
        records.records[0]
            .record
            .roots()
            .first()
            .unwrap()
            .pending_count(),
        1
    );
    runtime.close().await;
}

#[tokio::test]
async fn reconcile_receipt_reports_the_failed_pass_instead_of_waiting_for_a_later_scan() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Poll,
            message_board::WhenIdle::Hold,
            0,
            900,
        )
        .await;
    let runtime = fixture.runtime().await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
        .await;
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    let reconcile = || runtime.service.reconcile_reader(fixture.reader.clone());
    // The window scan itself fails: its progress table is unavailable.
    sqlx::query(
        "ALTER TABLE subscription_delivery_progress RENAME TO unavailable_delivery_progress",
    )
    .execute(&mut observer)
    .await
    .unwrap();
    assert!(
        matches!(reconcile().await, Err(_)),
        "a failed window scan must answer its reconciliation with the failure"
    );
    sqlx::query(
        "ALTER TABLE unavailable_delivery_progress RENAME TO subscription_delivery_progress",
    )
    .execute(&mut observer)
    .await
    .unwrap();
    // The pass ends before its scan: direct-message reload is unavailable.
    sqlx::query("ALTER TABLE router_pushes RENAME TO unavailable_router_pushes")
        .execute(&mut observer)
        .await
        .unwrap();
    assert!(
        matches!(reconcile().await, Err(_)),
        "a pass that ends before its scan must not leave reconciliation waiting"
    );
    sqlx::query("ALTER TABLE unavailable_router_pushes RENAME TO router_pushes")
        .execute(&mut observer)
        .await
        .unwrap();
    assert!(
        matches!(reconcile().await, Ok(())),
        "the next healthy pass answers reconciliation after its scan"
    );
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test]
async fn subscriptions_list_reconciles_a_subscriber_whose_scan_has_not_run() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Poll,
            message_board::WhenIdle::Hold,
            0,
            900,
        )
        .await;
    let runtime = fixture.runtime().await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
        .await;
    let identity =
        crate::ApplicationFixture::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
            .unwrap()
            .with_project_admission(fixture.admission.clone())
            .with_automation_store(fixture.automation.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone());
    let application = (identity).build();
    // Holding the owner's window-store mutex keeps the subscriber's scan from running.
    let guard = fixture.store.lock().await;
    fixture
        .post(&fixture.root, "Listed before the owner scanned it")
        .await;
    let reader = fixture.reader.clone();
    let mut listing = tokio::spawn(async move {
        application
            .board()
            .thread_subscriptions(ThreadSubscriptionsRequest { actor: reader })
            .await
    });
    (async {
        tokio::select! {
            listed = &mut listing => {
                let pending = listed.unwrap().map(|result| {
                    result
                        .subscriptions
                        .iter()
                        .map(|subscription| subscription.pending_count)
                        .collect::<Vec<_>>()
                });
                panic!("the list read local progress before reconciling its caller: {pending:?}");
            }
            () = runtime.service.reconcile_queued() => {}
        }
    })
    .await;
    drop(guard);
    let listed = (listing)
        .await
        .expect("listing task")
        .expect("listing succeeded")
        .subscriptions;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].pending_count, 1);
    runtime.close().await;
}
