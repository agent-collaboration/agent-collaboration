//! P1 diagnostic: actual per-target owner and SQLite, with no concurrent admissions.
#[path = "support/board_fixture.rs"]
// This scenario uses only the shared fixture's replay and shutdown paths.
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    unused_imports,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod board_fixture;
#[path = "support/held_queue_counter.rs"]
mod held_queue_counter;
#[path = "support/native_delivery.rs"]
mod native_delivery;
use automation_storage::AutomationStore;
use board_fixture::native_mail_owner;
use board_fixture::{BoardFixture, TestError};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MailItemId, MessageContent, MessageDelivery,
    PushDeliveryState, SessionMessageSendParams, SessionRef,
};
use collaboration_service::{
    ApplicationFixture, AttemptEvidenceSink, BoardAvailability, CollaborationApplication,
    DeliveryFuture, MachineIdentity, SessionMessageDelivery, SubscriptionDeliveryService,
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
    ) -> DeliveryFuture<'a, collaboration_service::NativeDeliveryResult> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(native_delivery::scripted(held())) })
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
fn held_input(target: &SessionRef, text: &str) -> Result<SessionMessageSendParams, TestError> {
    Ok(SessionMessageSendParams {
        item_id: Some(MailItemId::generate()),
        target: target.clone(),
        message: MessageContent::HumanUser {
            text: text.to_owned().try_into()?,
        },
        mode: MessageDelivery::Queue,
        generation_guard: None,
    })
}

async fn admit_held(
    app: &CollaborationApplication,
    target: &SessionRef,
    text: &str,
) -> Result<(), TestError> {
    let result = app
        .messages()
        .message_send(held_input(target, text)?)
        .await?;
    let collaboration_protocol::MessageSendResult::Local(result) = result else {
        return Err("local held admission returned remote sender custody".into());
    };
    assert_eq!(result.delivery_state, PushDeliveryState::Held);
    Ok(())
}

struct MeasuredAdmission {
    result: Result<
        collaboration_protocol::MessageSendResult,
        collaboration_service::collaboration_application::MessageFailure,
    >,
    metrics: serde_json::Value,
}

async fn measure_admission(
    app: &CollaborationApplication,
    counter: &held_queue_counter::SqlCounter,
    input: SessionMessageSendParams,
    depth: usize,
) -> MeasuredAdmission {
    let before = counter.offset();
    let started = Instant::now();
    let result = app.messages().message_send(input).await;
    let latency = started.elapsed();
    let statements = counter.since(before);
    assert!(!statements.is_empty(), "counter missed real SQL");
    MeasuredAdmission {
        result,
        metrics: json!({"depth":depth,"bodyBytes":4096,
        "sendMicros":latency.as_micros(),"statements":statements.len(),"queries":statements}),
    }
}

fn require_held(
    result: collaboration_protocol::MessageSendResult,
) -> Result<collaboration_protocol::PushMessageSendResult, TestError> {
    let collaboration_protocol::MessageSendResult::Local(result) = result else {
        return Err("local held admission returned remote sender custody".into());
    };
    if result.delivery_state != PushDeliveryState::Held {
        return Err(format!(
            "local admission must be Held, observed {:?}",
            result.delivery_state
        )
        .into());
    }
    Ok(result)
}

async fn assert_refused_without_custody(
    native: &native_mail_owner::NativeMailOwner,
    item: &MailItemId,
) -> Result<(), TestError> {
    assert!(
        native
            .interactions
            .get_push_record(&item.clone().try_into()?)
            .await?
            .is_none()
    );
    assert!(
        native
            .interactions
            .message_send_answer(item)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture and transport failures."
)]
async fn records_sequential_admission_costs_at_held_depths_and_the_real_capacity_boundary()
-> Result<(), TestError> {
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
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(
            fixture._directory.path().join("interaction.sqlite"),
        )
        .await?,
    );
    let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
            fixture
                ._admission
                .subscription_window_store(interactions.clone()),
        ))),
        push_store: interactions.clone(),
        delivery: route.clone(),
        presence: route.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(SystemSubscriptionClock),
    });
    service.start().await?;
    let identity = ApplicationFixture::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_automation_store(automation.clone())
    .with_subscription_delivery_service(service.clone(), route.clone());
    let (identity, native) = native_mail_owner::NativeMailOwner::start(
        identity,
        board_fixture::fixture_peer()?,
        collaboration_service::MailPushDeliveryProps {
            interactions,
            actors: service.clone(),
            presence: route.clone(),
            machine,
            clock: Arc::new(SystemSubscriptionClock),
        },
    )?;
    let app = (identity).build();
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"held-depth-target"}),
    )?;
    let reader = message_board::Identity::Session {
        session: target.clone().try_into()?,
    };
    let text = "p".repeat(4096);
    let mut current = 0;
    for depth in [0, 250, 1000] {
        while current < depth {
            admit_held(&app, &target, &text).await?;
            current += 1;
        }
        service.reconcile_reader(reader.clone()).await?;
        assert_eq!(
            native
                .interactions
                .list_held_push_records(&target)
                .await?
                .len(),
            depth
        );
        let mut measured =
            measure_admission(&app, &counter, held_input(&target, &text)?, depth).await;
        let admitted = require_held(measured.result?)?;
        assert_eq!(
            native
                .interactions
                .get_push_record(&admitted.push_id)
                .await?
                .unwrap()
                .body
                .as_deref(),
            Some(text.as_str())
        );
        let after_inspection = counter.offset();
        service.reconcile_reader(reader.clone()).await?;
        measured.metrics["excludedBarrierStatements"] =
            json!(counter.since(after_inspection).len());
        assert_eq!(
            route.0.load(Ordering::SeqCst),
            0,
            "unloaded input was injected"
        );
        eprintln!("HELD_DEPTH {}", measured.metrics);
        current += 1;
    }
    // The production byte cap and computed settlement reserve determine the last depth.
    let mut last_admitted = None;
    let (refused_input, refused_metrics) = loop {
        let input = held_input(&target, &text)?;
        let measured = measure_admission(&app, &counter, input.clone(), current).await;
        match measured.result {
            Ok(result) => {
                let admitted = require_held(result)?;
                last_admitted = Some((admitted.push_id, measured.metrics));
                current += 1;
            }
            Err(error) => {
                assert_eq!(error.kind, collaboration_service::collaboration_application::MessageFailureKind::MailboxFull);
                break (input, measured.metrics);
            }
        }
    };
    let (last_push, mut last_metrics) =
        last_admitted.expect("capacity did not extend beyond the 1000-depth measurement");
    let held = native.interactions.list_held_push_records(&target).await?;
    assert_eq!(held.len(), current);
    assert!(
        held.iter()
            .all(|record| record.body.as_deref() == Some(text.as_str()))
    );
    assert!(held.iter().any(|record| record.push_id == last_push));
    assert_eq!(
        route.0.load(Ordering::SeqCst),
        0,
        "unloaded input was injected"
    );
    last_metrics["boundary"] = json!("lastAdmitted");
    last_metrics["depthAfter"] = json!(current);
    eprintln!("HELD_DEPTH {last_metrics}");
    eprintln!("HELD_DEPTH {refused_metrics}");
    let refused_id = refused_input.item_id.clone().unwrap();
    assert_refused_without_custody(&native, &refused_id).await?;
    // Reusing the refused ID must still be a fresh MailboxFull refusal, never an SDK replay.
    let replay = measure_admission(&app, &counter, refused_input, current).await;
    let refusal = replay.result.unwrap_err();
    assert_eq!(
        refusal.kind,
        collaboration_service::collaboration_application::MessageFailureKind::MailboxFull
    );
    assert_refused_without_custody(&native, &refused_id).await?;
    assert_eq!(
        native
            .interactions
            .list_held_push_records(&target)
            .await?
            .len(),
        current
    );
    assert_eq!(
        route.0.load(Ordering::SeqCst),
        0,
        "unloaded input was injected"
    );
    eprintln!("HELD_DEPTH {}", replay.metrics);
    service.shutdown().await;
    drop(app);
    drop(service);
    native.close().await?;
    Arc::try_unwrap(automation)
        .ok()
        .expect("fixture retained scheduler store")
        .into_inner()
        .close()
        .await?;
    fixture.stop().await?;
    Ok(())
}
