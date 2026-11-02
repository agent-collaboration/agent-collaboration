//! Real public SDK reservation and callback fanout; only native I/O and the clock stand in.
use super::*;
use crate::{DebugSubscriptionClock, SubscriptionClock};
use collaboration_protocol::{PushHeaderFacts, PushKind};
use durable_steps::JournalScope;
use fleet_sync_delivery::{LocalItemAdmission, delivery_request_key};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, ItemId, SyncTime, TooLateAt};

async fn admit_local_notification(
    runtime: &Runtime,
    item: fleet_sync_protocol::DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
) {
    let config = runtime.fixture.host.config.local;
    let consumer = runtime.fixture.host.consumer.clone();
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                collaboration_runtime::project_received_mail(scope, &item, recorded_at())
                    .await
                    .map_err(StoreError::backend)?;
                let admission = Inbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(
                    scope,
                    config.owner,
                    config.limits,
                )
                .enqueue_local(
                    item,
                    &*consumer,
                    SyncTime::from_unix_millis(recorded_at().timestamp_millis()),
                )
                .await
                .map_err(StoreError::backend)?;
                let LocalItemAdmission::Attempt(attempt) = admission else {
                    return Err(StoreError::backend(
                        "fresh fixture notification did not admit",
                    ));
                };
                let key = delivery_request_key(&(attempt.item, attempt.attempt))
                    .map_err(StoreError::backend)?;
                scope
                    .admit::<ProjectionTask>(&key, &attempt)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_fresh_sdk_attempt_on_a_new_reader_is_not_a_previous_process_attempt() {
    let clock = Arc::new(DebugSubscriptionClock::new(recorded_at()));
    let runtime = Runtime::new_with_clock(Some(clock.clone())).await;
    let mail = Arc::new(runtime.local_mail_runtime());
    mail.attach_reader_readiness().unwrap();
    let item = follow(&runtime, 2, 10);
    let id = item.id();
    runtime.client.script(id, vec![accepted()]);
    admit_local_notification(&runtime, item).await;
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        runtime.client.calls(id),
        1,
        "a fresh notification must reach native I/O immediately"
    );
    assert_eq!(
        mail.local_outcome_for_test(id).await.unwrap(),
        Some(RecipientOutcome::Delivered {
            acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
        })
    );
    assert_eq!(
        clock.now(),
        recorded_at(),
        "the first effect needs no clock advance"
    );
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn reservation_excludes_a_cancelled_member_and_refreezes_the_remaining_listing() {
    assert_reservation_refreezes_after_cancellations(1).await;
}

#[tokio::test]
async fn reservation_with_no_deliverable_members_is_known_unsent_without_native_io() {
    assert_reservation_refreezes_after_cancellations(2).await;
}

async fn assert_reservation_refreezes_after_cancellations(cancelled: usize) {
    let clock = Arc::new(DebugSubscriptionClock::new(recorded_at()));
    let runtime = Runtime::new_with_clock(Some(clock)).await;
    let mail = Arc::new(runtime.local_mail_runtime());
    mail.attach_reader_readiness().unwrap();
    let first = follow(&runtime, 2, 10);
    let second = follow(&runtime, 20, 30);
    let ids = [first.id(), second.id()];
    let target: collaboration_protocol::SessionRef =
        first.recipient().address.clone().try_into().unwrap();
    let cancels = [&first, &second].map(|item| {
        DeliveryCancel::new(
            ItemId::new(uuid::Uuid::now_v7()).unwrap(),
            item.sender().clone(),
            item.recipient().clone(),
            None,
            CancelBody { target: item.id() },
        )
        .unwrap()
    });
    runtime
        .fixture
        .owner
        .record_notification_delivery(&target, recorded_at())
        .await
        .unwrap();
    admit_local_notification(&runtime, first).await;
    admit_local_notification(&runtime, second).await;
    mail.drive_due_for_test().await.unwrap();
    let member_ids =
        ids.map(|id| collaboration_protocol::PushId::try_from(id.as_uuid().to_string()).unwrap());
    let listing_id =
        collaboration_protocol::PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    let listing = runtime
        .fixture
        .owner
        .freeze_notification_listing(
            collaboration_protocol::PushRecordDraft {
                push_id: listing_id.clone(),
                kind: PushKind::NotificationListing,
                origin: collaboration_protocol::PushOrigin::Router(PushKind::NotificationListing),
                origin_router_ref: Some(
                    collaboration_protocol::RouterOriginRef::NotificationListing {
                        target: target.clone(),
                        listing_id: listing_id.clone(),
                    }
                    .canonical_string()
                    .unwrap(),
                ),
                target: target.clone(),
                mode: None,
                guard: None,
                reply_to_push_id: None,
                header_facts: PushHeaderFacts::NotificationListing { item_count: 2 },
                body: Some(format!(
                    "{}\n{}",
                    member_ids[0].as_str(),
                    member_ids[1].as_str()
                )),
                activity: None,
                created_at: recorded_at(),
            },
            &member_ids,
        )
        .await
        .unwrap();
    // The freeze committed, but the public SDK cancellation wins before reservation.
    for cancel in cancels.into_iter().take(cancelled) {
        mail.cancel_local(
            cancel,
            collaboration_mail_protocol::CancelCause::ByRequester {
                by: serde_json::from_value(
                    serde_json::json!({"kind":"human","humanId":"reservation-proof"}),
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
    }
    assert!(matches!(
        mail.local_outcome_for_test(ids[0]).await.unwrap(),
        Some(RecipientOutcome::Prevented { .. })
    ));
    let reserved = mail.reserve_notification_listing(&listing).await;
    assert!(
        reserved.is_ok(),
        "a non-Deliver member must be excluded without aborting eligible members: {reserved:?}"
    );
    let current = runtime
        .fixture
        .owner
        .get_push_record(&listing_id)
        .await
        .unwrap()
        .unwrap();
    if cancelled == 1 {
        assert!(matches!(
            current.header_facts,
            PushHeaderFacts::NotificationListing { item_count: 1 }
        ));
        assert!(reserved.unwrap().is_some());
    } else {
        assert!(reserved.unwrap().is_none());
        assert_eq!(
            current.delivery_state,
            collaboration_protocol::PushDeliveryState::Rejected
        );
        assert!(matches!(
            current.last_outcome.as_ref().unwrap().outcome,
            DeliveryOutcome::NotSubmitted { .. }
        ));
    }
    let (members, next) = runtime
        .fixture
        .owner
        .notification_listing_members(&target, &listing_id, None, 50)
        .await
        .unwrap();
    assert!(next.is_none());
    assert_eq!(
        members
            .iter()
            .map(|record| record.push_id.clone())
            .collect::<Vec<_>>(),
        member_ids
            .iter()
            .skip(cancelled)
            .cloned()
            .collect::<Vec<_>>()
    );
    if cancelled == 1 {
        assert!(
            !current
                .body
                .as_ref()
                .unwrap()
                .contains(member_ids[0].as_str())
        );
        assert!(
            current
                .body
                .as_ref()
                .unwrap()
                .contains(member_ids[1].as_str())
        );
    }
    assert!(
        runtime.client.all_calls().is_empty(),
        "refreeze precedes native I/O"
    );
    assert!(matches!(
        mail.local_outcome_for_test(ids[0]).await.unwrap(),
        Some(RecipientOutcome::Prevented { .. })
    ));
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn listing_reserves_sdk_members_before_native_io_and_replays_one_receipt_to_each_callback() {
    let clock = Arc::new(DebugSubscriptionClock::new(recorded_at()));
    let runtime = Runtime::new_with_clock(Some(clock.clone())).await;
    let mail = Arc::new(runtime.local_mail_runtime());
    mail.attach_reader_readiness().unwrap();
    let first = follow(&runtime, 2, 10);
    let second = follow(&runtime, 20, 30);
    let ids = [first.id(), second.id()];
    let target: collaboration_protocol::SessionRef =
        first.recipient().address.clone().try_into().unwrap();
    let cancel_id = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let cancel = DeliveryCancel::new(
        cancel_id,
        first.sender().clone(),
        first.recipient().clone(),
        None,
        CancelBody { target: ids[0] },
    )
    .unwrap();
    runtime
        .fixture
        .owner
        .record_notification_delivery(&target, recorded_at())
        .await
        .unwrap();
    admit_local_notification(&runtime, first).await;
    admit_local_notification(&runtime, second).await;
    mail.drive_due_for_test().await.unwrap();
    for id in ids {
        assert_eq!(
            mail.local_outcome_for_test(id).await.unwrap(),
            Some(RecipientOutcome::Held)
        );
        assert_eq!(runtime.client.calls(id), 0);
    }
    let gate = runtime
        .client
        .pause_next_unlisted_result(collaboration_contracts::scripted_delivery_result(accepted()));
    let actors = runtime.actors.clone();
    let now = recorded_at() + chrono::Duration::seconds(600);
    let advance = tokio::spawn(async move { actors.debug_advance_notification_clock(now).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered)
        .await
        .expect("listing did not reach native I/O")
        .unwrap();
    let calls = runtime.client.all_calls();
    assert_eq!(calls.len(), 1);
    let listing = runtime
        .fixture
        .owner
        .get_push_record(&calls[0].payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(listing.kind, PushKind::NotificationListing);
    assert!(matches!(
        listing.header_facts,
        PushHeaderFacts::NotificationListing { item_count: 2 }
    ));
    // The committed public cancel must defer while the listing's native effect is open.
    mail.cancel_local(
        cancel,
        collaboration_mail_protocol::CancelCause::ByRequester {
            by: serde_json::from_value(
                serde_json::json!({"kind":"human","humanId":"listing-proof"}),
            )
            .unwrap(),
        },
    )
    .await
    .unwrap();
    assert_eq!(mail.local_outcome_for_test(cancel_id).await.unwrap(), None);
    // The public SDK status intentionally reports Held for an Attempting retry;
    // the pending cancel above is the decisive proof that prevention was deferred.
    assert_eq!(
        mail.local_outcome_for_test(ids[0]).await.unwrap(),
        Some(RecipientOutcome::Held)
    );
    let callbacks = mail.clone();
    let callbacks = tokio::spawn(async move { callbacks.drive_due_for_test().await });
    gate.release.send(()).unwrap();
    advance.await.unwrap().unwrap();
    callbacks.await.unwrap().unwrap();
    mail.drive_due_for_test().await.unwrap();
    for id in ids {
        assert_eq!(
            mail.local_outcome_for_test(id).await.unwrap(),
            Some(RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
            })
        );
        assert_eq!(runtime.client.calls(id), 0);
    }
    assert!(matches!(
        mail.local_outcome_for_test(cancel_id).await.unwrap(),
        Some(RecipientOutcome::TooLate {
            at: TooLateAt::Delivered,
            ..
        })
    ));
    assert_eq!(runtime.client.all_calls().len(), 1);
    assert_eq!(clock.now(), now);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn known_unsent_listing_releases_sdk_custody_and_retries_once_when_presence_is_rechecked() {
    let clock = Arc::new(DebugSubscriptionClock::new(recorded_at()));
    let runtime = Runtime::new_with_clock(Some(clock)).await;
    let mail = Arc::new(runtime.local_mail_runtime());
    mail.attach_reader_readiness().unwrap();
    let item = follow(&runtime, 2, 10);
    let id = item.id();
    let target: collaboration_protocol::SessionRef =
        item.recipient().address.clone().try_into().unwrap();
    runtime
        .fixture
        .owner
        .record_notification_delivery(&target, recorded_at())
        .await
        .unwrap();
    admit_local_notification(&runtime, item).await;
    mail.drive_due_for_test().await.unwrap();
    let gate = runtime.client.pause_next_unlisted_result(
        collaboration_contracts::NativeDeliveryResult::not_submitted(
            "fixture definite unsent",
            true,
            collaboration_contracts::UnsentCause::Holdable {
                otherwise: collaboration_contracts::NativeRefusalCode::CarrierUnavailable,
            },
        ),
    );
    let actors = runtime.actors.clone();
    let due = recorded_at() + chrono::Duration::seconds(600);
    let advance = tokio::spawn(async move { actors.debug_advance_notification_clock(due).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered)
        .await
        .expect("listing did not reach native I/O")
        .unwrap();
    gate.release.send(()).unwrap();
    advance.await.unwrap().unwrap();
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        mail.local_outcome_for_test(id).await.unwrap(),
        Some(RecipientOutcome::Held)
    );
    let push = id.as_uuid().to_string().try_into().unwrap();
    assert!(
        runtime
            .fixture
            .owner
            .notification_listing_for_member(&push)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    runtime
        .actors
        .debug_advance_notification_clock(due)
        .await
        .unwrap();
    assert_eq!(
        runtime.client.all_calls().len(),
        1,
        "no native retry before the existing presence deadline"
    );
    assert_eq!(
        runtime
            .fixture
            .owner
            .notification_window(&target)
            .await
            .unwrap()
            .unwrap()
            .last_delivered_at,
        recorded_at()
    );
    let gate = runtime
        .client
        .pause_next_unlisted_result(collaboration_contracts::scripted_delivery_result(accepted()));
    let actors = runtime.actors.clone();
    let retry = tokio::spawn(async move {
        actors
            .debug_advance_notification_clock(due + chrono::Duration::seconds(30))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered)
        .await
        .expect("released member did not retry")
        .unwrap();
    gate.release.send(()).unwrap();
    retry.await.unwrap().unwrap();
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        mail.local_outcome_for_test(id).await.unwrap(),
        Some(RecipientOutcome::Delivered {
            acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
        })
    );
    assert_eq!(runtime.client.all_calls().len(), 2);
    assert_eq!(runtime.client.calls(id), 0);
    drop(mail);
    runtime.close().await;
}
