//! A permanent provider rejection must finish one wake delivery without retry eligibility.

use crate::AutomationConfigurationHandle;
use crate::wakeup_delivery_sender::WakeDeliverySender;
use agent_automation::{ExpiryRule, OperationId, TimingRule};
use automation_storage::{AutomationStore, WakeCreate, WakeEvaluation};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_protocol::{
    CodexGeneration, DeliveryNextAction, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, SavedMessage, SessionReachability, SessionRef,
};
use runtime_primitives::Clock;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

struct ProviderSessionNotFound;

impl SessionMessageDelivery for ProviderSessionNotFound {
    fn deliver<'a>(
        &'a self,
        _request: collaboration_contracts::layer_zero::DeliveryRequest,
        _evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async {
            Ok(collaboration_contracts::scripted_delivery_result(DeliveryReceipt {
                outcome: DeliveryOutcome::Rejected(DeliveryRejection {
                    reason: DeliveryRejectionReason::ProviderSessionNotFound,
                    next_action: DeliveryNextAction::CorrectRequest,
                    client_code: Some(-32002),
                    detail: Some("this session never started a turn and did not survive the provider restart; create a new conversation".to_owned()),
                    claims: None,
                    native_failure: None,

                    }),
                reachability: Some(SessionReachability::ProviderAcp),
                client: None,
            }))
        })
    }

    fn reconcile_attempt(
        &self,
        _context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::KnownNotSubmitted) })
    }
}

async fn dispatch_fixture_wake(
    delivery: Arc<dyn SessionMessageDelivery>,
    mode: collaboration_protocol::MessageDelivery,
) -> automation_storage::DeliveryRecord<SessionRef, CodexGeneration, serde_json::Value> {
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1792432800000,
        ))
        .expect("fixed wake sender clock"),
    );
    let root = tempfile::tempdir().expect("automation root");
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&root.path().join("automation.sqlite"))
            .await
            .expect("automation store"),
    ));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
            .await
            .expect("interaction store"),
    );
    let now_ms = clock.now().unix_millis() - 10_000;
    let message: SavedMessage = serde_json::from_value(json!({
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"empty-session"},
        "content":{"kind":"humanUser","text":"wake fixture"},
        "delivery":mode
    }))
    .expect("saved message without an explicit generation guard");
    let wake = store
        .lock()
        .await
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message,
            timing: TimingRule::After { seconds: 1 },
            expiry: ExpiryRule::None,
            now_ms,
        })
        .await
        .expect("wake creation");
    let wakeup_id = wake.definition.wakeup_id;
    let delivery_id = {
        let mut store = store.lock().await;
        let due_ids = store
            .due_wakeup_ids(now_ms + 5_000, 4)
            .await
            .expect("due wake inventory");
        assert_eq!(due_ids.as_slice(), std::slice::from_ref(&wakeup_id));
        match store
            .evaluate_wakeup::<SavedMessage>(
                &wakeup_id,
                now_ms + 5_000,
                super::build_wake_push_draft,
            )
            .await
            .expect("wake evaluation")
        {
            WakeEvaluation::Fired { delivery_id, .. } => delivery_id,
            _ => panic!("eligible wake did not fire"),
        }
    };
    crate::PushHandoffWorker::new(Arc::clone(&store), Arc::clone(&interactions))
        .apply_pending()
        .await
        .expect("wake push handoff");
    let sender = WakeDeliverySender {
        clock: clock.clone(),
        interactions,
        delivery,
        configuration: AutomationConfigurationHandle::default(),
        machine_identity: collaboration_contracts::MachineIdentity::new(
            wake.definition.message.target.endpoint.service_id.clone(),
            None,
        )
        .expect("machine identity"),
    };

    sender
        .dispatch(Arc::clone(&store), delivery_id.clone())
        .await
        .expect("wake dispatch");

    let record = store
        .lock()
        .await
        .read_delivery::<SessionRef, CodexGeneration, serde_json::Value>(&delivery_id)
        .await
        .expect("delivery record");
    assert!(
        store
            .lock()
            .await
            .eligible_delivery_ids(clock.now().unix_millis() + 60_000, 4)
            .await
            .expect("future eligible deliveries")
            .is_empty()
    );
    record
}

#[tokio::test]
async fn provider_session_not_found_finishes_wake_after_one_attempt() {
    let record = dispatch_fixture_wake(
        Arc::new(ProviderSessionNotFound),
        collaboration_protocol::MessageDelivery::Auto,
    )
    .await;
    assert_eq!(
        serde_json::to_value(record.status).expect("status"),
        json!("failed")
    );
    assert_eq!(record.attempt.as_ref().expect("attempt").attempt_number, 1);
    assert_eq!(
        record.receipt.as_ref().expect("receipt")["outcome"]["reason"],
        "providerSessionNotFound"
    );
    assert_eq!(
        record.receipt.as_ref().expect("receipt")["outcome"]["clientCode"],
        -32002
    );
}

struct QueueUnsupportedRoute;

impl collaboration_contracts::SessionDeliveryRoute for QueueUnsupportedRoute {
    fn reachability(&self) -> SessionReachability {
        SessionReachability::ProviderAcp
    }
    fn supports_delivery_mode(
        &self,
        _: &SessionRef,
        mode: collaboration_protocol::MessageDelivery,
    ) -> bool {
        mode != collaboration_protocol::MessageDelivery::Queue
    }
    fn claim(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::NativeRouteClaim> {
        Box::pin(async { Ok(collaboration_contracts::NativeRouteClaim::holds()) })
    }
    fn presence(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::RoutePresence> {
        Box::pin(async { Ok(collaboration_contracts::RoutePresence::Running) })
    }
    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        assert_eq!(request.mode, collaboration_protocol::MessageDelivery::Steer);
        assert!(request.payload.line.as_str().contains("wake fixture"));
        Box::pin(async {
            Ok(collaboration_contracts::scripted_delivery_result(
                DeliveryReceipt {
                    outcome: DeliveryOutcome::Steered,
                    reachability: Some(SessionReachability::ProviderAcp),
                    client: None,
                },
            ))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

#[tokio::test]
async fn queued_wake_without_queue_support_records_one_accepted_steer() {
    let delivery = Arc::new(crate::SessionDeliveryRouter::new(vec![Arc::new(
        QueueUnsupportedRoute,
    )]));
    let record =
        dispatch_fixture_wake(delivery, collaboration_protocol::MessageDelivery::Queue).await;
    assert_eq!(record.status, agent_automation::DeliveryStatus::Accepted);
    assert_eq!(record.attempt.as_ref().expect("attempt").attempt_number, 1);
    let receipt: DeliveryReceipt =
        serde_json::from_value(record.receipt.expect("receipt")).expect("typed receipt");
    assert_eq!(receipt.outcome, DeliveryOutcome::Steered);
}
