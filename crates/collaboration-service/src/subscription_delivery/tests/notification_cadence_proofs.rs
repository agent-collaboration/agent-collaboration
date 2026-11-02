//! Real reader/store cadence with a controlled clock; only the external client stands in.
use super::*;
use crate::SubscriptionClock;
use collaboration_protocol::{PushHeaderFacts, PushOrigin, PushRecordDraft, RouterOriginRef};
fn at(ms: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(ms).unwrap()
}
async fn held_expiry(runtime: &Runtime) -> PushId {
    let recipient = request("reader-a", MessageDelivery::Auto).target;
    let id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    let scope = message_board::SubscriptionScope::thread(message_board::ThreadId::generate());
    runtime
        .store
        .insert_push_record(PushRecordDraft {
            push_id: id.clone(),
            kind: PushKind::SubscriptionExpiry,
            origin: PushOrigin::Router(PushKind::SubscriptionExpiry),
            origin_router_ref: Some(
                RouterOriginRef::SubscriptionExpiry {
                    target: recipient.clone(),
                    scope,
                    subscription_generation: message_board::SubscriptionGeneration::new(1).unwrap(),
                }
                .canonical_string()
                .unwrap(),
            ),
            target: recipient,
            mode: None,
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::SubscriptionExpiry {
                scope: "thread fixture".into(),
            },
            body: Some("Renew subscription".into()),
            activity: None,
            created_at: at(0),
        })
        .await
        .unwrap();
    runtime.store.mark_push_attempted(&id).await.unwrap();
    runtime
        .store
        .hold_push_record(
            &id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "recipient window".into(),
                },
                reachability: None,
                client: None,
            },
        )
        .await
        .unwrap();
    id
}
impl Runtime {
    async fn restart_reader_service(&mut self, clock: Arc<dyn crate::SubscriptionClock>) {
        self.service.shutdown().await;
        let (calls, receiver) = mpsc::unbounded_channel();
        let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: self.store.clone(),
            delivery: Arc::new(HeldClient {
                calls,
                supports_queue: false,
            }),
            presence: Arc::new(RunningPresence),
            machine_identity: MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                Some("Native attempt test"),
            )
            .unwrap(),
            clock,
        });
        self.observations = service.observe_owners();
        self.calls = receiver;
        self.service = service;
        self.service.start().await.unwrap();
    }
}

#[tokio::test]
async fn a_reader_recovers_its_interrupted_listing_before_a_fresh_native_effect() {
    let clock = Arc::new(crate::DebugSubscriptionClock::new(at(0)));
    let mut runtime = Runtime::with_clock(clock.clone()).await;
    let member = held_expiry(&runtime).await;
    let target = request("reader-a", MessageDelivery::Auto).target;
    let id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    runtime
        .store
        .freeze_notification_listing(
            PushRecordDraft {
                push_id: id.clone(),
                kind: PushKind::NotificationListing,
                origin: PushOrigin::Router(PushKind::NotificationListing),
                origin_router_ref: Some(
                    RouterOriginRef::NotificationListing {
                        target: target.clone(),
                        listing_id: id.clone(),
                    }
                    .canonical_string()
                    .unwrap(),
                ),
                target,
                mode: None,
                guard: None,
                reply_to_push_id: None,
                header_facts: PushHeaderFacts::NotificationListing { item_count: 1 },
                body: Some("one frozen member".into()),
                activity: None,
                created_at: at(0),
            },
            std::slice::from_ref(&member),
        )
        .await
        .unwrap();
    runtime.store.mark_push_attempted(&id).await.unwrap();
    runtime.restart_reader_service(clock).await;
    let receipt = runtime
        .service
        .attempt_mail_once(
            PushKind::BoardNotification,
            request("reader-a", MessageDelivery::Auto),
        )
        .await;
    assert!(matches!(
        receipt.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(
        runtime.calls.try_recv().is_err(),
        "recovery must precede this reader's first native effect"
    );
    assert_eq!(
        runtime
            .store
            .get_push_record(&id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::OutcomeUnknown
    );
    assert_eq!(
        runtime
            .store
            .get_push_record(&member)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::OutcomeUnknown
    );
    runtime.close().await;
}
#[tokio::test]
async fn one_listing_covers_overflow_and_the_next_rolling_window_survives_reader_restart() {
    let clock = Arc::new(crate::DebugSubscriptionClock::new(at(0)));
    let mut runtime = Runtime::with_clock(clock.clone()).await;
    let service = runtime.service.clone();
    let first = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::BoardNotification,
                request("reader-a", MessageDelivery::Auto),
            )
            .await
    });
    runtime.next_call().await.reply.send(started()).unwrap();
    assert_eq!(
        first.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );
    let mut members = Vec::new();
    for _ in 0..55 {
        members.push(held_expiry(&runtime).await);
    }
    let hold = runtime
        .service
        .attempt_mail_once(
            PushKind::BoardNotification,
            request("reader-a", MessageDelivery::Auto),
        )
        .await;
    assert!(matches!(
        hold.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(runtime.calls.try_recv().is_err());
    let service = runtime.service.clone();
    let flush =
        tokio::spawn(async move { service.debug_advance_notification_clock(at(600_000)).await });
    let call = runtime.next_call().await;
    let listing = runtime
        .store
        .get_push_record(&call.request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        listing.header_facts,
        PushHeaderFacts::NotificationListing { item_count: 55 }
    ));
    let body = listing.body.as_ref().unwrap();
    assert_eq!(body.matches("router://").count(), 50);
    assert!(body.contains("notification_members"));
    assert!(body.contains("55 notifications total"));
    call.reply.send(started()).unwrap();
    flush.await.unwrap().unwrap();
    for member in &members {
        assert_eq!(
            runtime
                .store
                .get_push_record(member)
                .await
                .unwrap()
                .unwrap()
                .delivery_state,
            collaboration_protocol::PushDeliveryState::Delivered
        );
    }
    assert!(runtime.calls.try_recv().is_err());
    clock.debug_advance_to(at(600_001)).unwrap();
    let last = held_expiry(&runtime).await;
    let held = runtime
        .service
        .attempt_mail_once(
            PushKind::BoardNotification,
            request("reader-a", MessageDelivery::Auto),
        )
        .await;
    assert!(matches!(
        held.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    runtime.restart_reader_service(clock).await;
    runtime
        .service
        .debug_advance_notification_clock(at(1_199_999))
        .await
        .unwrap();
    assert!(runtime.calls.try_recv().is_err());
    let service = runtime.service.clone();
    let flush = tokio::spawn(async move {
        service
            .debug_advance_notification_clock(at(1_200_000))
            .await
    });
    let call = runtime.next_call().await;
    let listing = runtime
        .store
        .get_push_record(&call.request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        listing.header_facts,
        PushHeaderFacts::NotificationListing { item_count: 1 }
    ));
    call.reply.send(started()).unwrap();
    flush.await.unwrap().unwrap();
    assert_eq!(
        runtime
            .store
            .get_push_record(&last)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::Delivered
    );
    assert!(runtime.calls.try_recv().is_err());
    runtime.close().await;
}
