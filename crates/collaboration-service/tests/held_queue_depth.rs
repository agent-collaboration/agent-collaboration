//! P1 diagnostic: actual per-target owner and SQLite, with no concurrent admissions.
#[path = "support/board_fixture.rs"]
#[allow(dead_code)] // This scenario uses only the shared fixture's replay and shutdown paths.
mod board_fixture;
#[path = "support/held_queue_counter.rs"]
mod held_queue_counter;
use automation_storage::{AutomationStore, LocalMessageRequest};
use board_fixture::{BoardFixture, TestError};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MailItemId, MessageContent, MessageDelivery,
    PushDeliveryState, PushHeaderFacts, PushKind, PushOrigin, PushRecordDraft,
    SessionMessageSendParams, SessionRef,
};
use collaboration_service::{
    AttemptEvidenceSink, BoardAvailability, CollaborationApplication, DeliveryFuture,
    MachineIdentity, ServiceIdentity, SessionMessageDelivery, SubscriptionDeliveryService,
    SubscriptionDeliveryServiceProps, SystemSubscriptionClock, TargetPresence, TargetPresenceProbe,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio::sync::Mutex;

#[derive(Default)]
struct UnloadedTarget(AtomicUsize);
impl TargetPresenceProbe for UnloadedTarget {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Wakeable) })
    }
}
impl SessionMessageDelivery for UnloadedTarget {
    fn reconcile_attempt(
        &self,
        _: collaboration_service::AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, collaboration_service::AttemptReconciliation> {
        Box::pin(async { Ok(collaboration_service::AttemptReconciliation::StillUnknown) })
    }
    fn deliver<'a>(
        &'a self,
        _: collaboration_service::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, DeliveryReceipt> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(held()) })
    }
}
fn held() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "unloaded depth fixture".into(),
        },
        reachability: None,
        client: None,
    }
}
fn draft(target: &SessionRef, text: &str) -> Result<LocalMessageRequest, TestError> {
    Ok(LocalMessageRequest {
        draft: PushRecordDraft {
            push_id: MailItemId::generate().try_into()?,
            kind: PushKind::DirectMessage,
            origin: PushOrigin::OwnerUnverified,
            origin_router_ref: None,
            target: target.clone(),
            mode: Some(MessageDelivery::Auto),
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some(text.into()),
            activity: None,
            created_at: chrono::Utc::now(),
        },
        target_identity: "depth fixture".into(),
    })
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn records_sequential_admission_costs_at_four_held_depths() -> Result<(), TestError> {
    let counter = held_queue_counter::SqlCounter::default();
    counter.install();
    let fixture = BoardFixture::start_replay().await?;
    let automation = Arc::new(Mutex::new(
        AutomationStore::open(&fixture._directory.path().join("automation.sqlite")).await?,
    ));
    let route = Arc::new(UnloadedTarget::default());
    let machine = MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        Some("P1 real-store fixture"),
    )?;
    let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
            fixture
                ._admission
                .subscription_window_store(automation.clone()),
        ))),
        push_store: automation.clone(),
        delivery: route.clone(),
        presence: route.clone(),
        machine_identity: machine,
        clock: Arc::new(SystemSubscriptionClock),
    });
    service.start().await?;
    let identity = ServiceIdentity::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_automation_store(automation.clone())
    .with_subscription_delivery_service(service.clone(), route.clone());
    let app = CollaborationApplication::new(identity);
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"held-depth-target"}),
    )?;
    let reader = message_board::Identity::Session {
        session: target.clone().try_into()?,
    };
    let text = "p".repeat(4096);
    let mut current = 0;
    for depth in [0, 250, 1000, 2500] {
        while current < depth {
            let request = draft(&target, &text)?;
            let mut store = automation.lock().await;
            store.prepare_local_message(&request).await?;
            store
                .hold_unsent_direct_message(&request.draft.push_id, held())
                .await?;
            current += 1;
        }
        // Real owner acknowledgement follows its DM pass, excluding restore work.
        service.reconcile_reader(reader.clone()).await?;
        let before = counter.offset();
        let start = Instant::now();
        let result = app
            .messages()
            .message_send(SessionMessageSendParams {
                item_id: Some(MailItemId::generate()),
                target: target.clone(),
                message: MessageContent::HumanUser {
                    text: text.clone().try_into()?,
                },
                mode: MessageDelivery::Auto,
                generation_guard: None,
            })
            .await?;
        let latency = start.elapsed();
        assert_eq!(result.delivery_state, PushDeliveryState::Held);
        let statements = counter.since(before);
        assert!(!statements.is_empty(), "counter missed real SQL");
        // Count the admission itself; the following acknowledgement is separate.
        let after_admission = counter.offset();
        service.reconcile_reader(reader.clone()).await?;
        let barrier_statements = counter.since(after_admission).len();
        eprintln!(
            "HELD_DEPTH {}",
            json!({"depth":depth,"bodyBytes":text.len(),"sendMicros":latency.as_micros(),"statements":statements.len(),"queries":statements,"excludedBarrierStatements":barrier_statements})
        );
        current += 1;
    }
    assert_eq!(
        route.0.load(Ordering::SeqCst),
        0,
        "unloaded input was injected"
    );
    service.shutdown().await;
    drop(app);
    drop(service);
    fixture.stop().await?;
    Ok(())
}
