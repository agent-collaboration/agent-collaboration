//! A queued input's outcome (D1 R12, review F2): waiting in the session's
//! line is not a dispatch that failed to start, so no start deadline applies.
//! Time moves only on the port's one injected clock (review C1).

use super::HostSessionCommandPort;
use crate::{
    ExternalProviderSupervisor, LiveSessionOwnership, LiveSessionOwnershipCheck,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::{
    CommandFailure, DeliveryFuture, QueuedInputOutcome, SessionCommandPort as _,
    SessionTargetCommand,
};
use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, NonEmptyText, OperationId,
    ProviderBindingId, ProviderBindingIdentity, ProviderKind, ProviderRuntimeIdentity,
    ProviderTransport, SessionId, SessionRef,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use session_event_model::{InputId, PromptContent, SessionEvent, StopReason, TurnOutcome};
use std::{sync::Arc, task::Poll, time::Duration};
use tokio::sync::Mutex;

struct NoLivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
                .to_owned()
                .try_into()
                .expect("service ID"),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint ID"),
        },
        session_id: SessionId::try_from("queued-session".to_owned()).expect("session ID"),
    }
}

fn binding(target: &SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("queued-binding".to_owned()).expect("binding"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("queued-fixture".to_owned()).expect("name"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80"
                .to_owned()
                .try_into()
                .expect("service epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: collaboration_protocol::ProviderCapabilities::try_from(vec![
            collaboration_protocol::ProviderCapability {
                name: collaboration_protocol::ProviderCapabilityName::Prompt,
                status: collaboration_protocol::ProviderCapabilityStatus::Supported,
                evidence: collaboration_protocol::ProviderCapabilityEvidence::Advertised,
            },
        ])
        .expect("capabilities"),
    }
}

struct StoreDeadlineFixture {
    _root: tempfile::TempDir,
    store: Arc<Mutex<ProviderOperationStore>>,
    clock: Arc<router_clock::ManualClock>,
    port: HostSessionCommandPort,
    session: message_board::SessionRef,
}

impl StoreDeadlineFixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .expect("store"),
        ));
        let target = target();
        let session = serde_json::from_value(serde_json::to_value(&target).expect("target JSON"))
            .expect("hub session");
        let hub = Arc::new(ProviderSessionEventHub::new(Arc::clone(&store)));
        let supervisor = Arc::new(
            ExternalProviderSupervisor::new(Vec::new(), Arc::clone(&store)).expect("supervisor"),
        );
        let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
        let delivery = Arc::new(ProviderAcpDeliveryRoute::new(
            target.endpoint.service_id.clone(),
            std::collections::HashSet::new(),
            collaboration_runtime::EndpointDirectory::new(target.endpoint.service_id),
            Arc::clone(&supervisor),
            Arc::clone(&store),
            Arc::new(NoLivePeer),
            clock.clone(),
        ));
        let port = HostSessionCommandPort::new(
            supervisor,
            delivery,
            hub,
            Arc::clone(&store),
            clock.clone(),
        );
        Self {
            _root: root,
            store,
            clock,
            port,
            session,
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn load_resume_and_close_answer_at_l_when_the_port_pre_read_is_held() {
    let fixture = StoreDeadlineFixture::new().await;
    let mut store = fixture.store.lock().await;
    let command = || SessionTargetCommand {
        target: fixture.session.clone(),
        actor: serde_json::from_value(
            serde_json::json!({"kind":"human","humanId":"fixture-owner"}),
        )
        .expect("actor"),
    };
    for mut pending in [
        fixture.port.load_session(command()),
        fixture.port.resume_session(command()),
        fixture.port.close(command()),
    ] {
        assert!(futures_util::poll!(&mut pending).is_pending());
        fixture
            .clock
            .advance(Duration::from_secs(10))
            .expect("reach L");
        let Poll::Ready(Err(error)) = futures_util::poll!(&mut pending) else {
            panic!("the Host command must answer at L while its pre-read is held");
        };
        assert_eq!(error.to_string(), "the store didn't respond in time");
        let failure = error.operation_failure().expect("typed operation failure");
        assert_eq!(
            failure.stage,
            collaboration_protocol::ConversationOperationFailureStage::Admission
        );
        assert_eq!(
            failure.effect,
            collaboration_protocol::ProviderOperationEffect::None
        );
        let operation = failure.operation_id.as_ref().expect("operation ID");
        assert!(
            store
                .inspect(operation)
                .await
                .expect("inspect operation")
                .is_none()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn activation_event_budget_includes_the_first_history_attach() {
    let fixture = StoreDeadlineFixture::new().await;
    let _store = fixture.store.lock().await;
    let activation = fixture
        .port
        .wait_for_activation(fixture.session.clone(), None);
    tokio::pin!(activation);
    assert!(futures_util::poll!(&mut activation).is_pending());
    fixture
        .clock
        .advance(Duration::from_secs(30))
        .expect("reach the activation deadline");
    assert!(matches!(
        futures_util::poll!(&mut activation),
        Poll::Ready(Err(CommandFailure::ProviderFailure))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn restore_settlement_fallback_without_a_live_entry_is_bounded_by_l() {
    let fixture = StoreDeadlineFixture::new().await;
    let _store = fixture.store.lock().await;
    let wait = fixture
        .port
        .wait_for_restore_settlement(OperationId::generate());
    tokio::pin!(wait);
    assert!(futures_util::poll!(&mut wait).is_pending());
    fixture
        .clock
        .advance(Duration::from_secs(10))
        .expect("reach the fallback deadline");
    let Poll::Ready(Err(error)) = futures_util::poll!(&mut wait) else {
        panic!("a restore whose live result is gone must answer at L");
    };
    assert_eq!(error.to_string(), "the store didn't respond in time");
}

/// Oracle: D1 spec R12, "a queued ACP prompt answers when its own turn ends,
/// or with NotSent and the reason", and review F2: an input still waiting
/// behind a turn that outlasts the 30 s turn-start deadline is still waiting,
/// not failed, and resolves Started when its own turn starts. It is asked
/// through the port's queue_outcome; time passes on the port's injected
/// clock, the clock its turn-start deadline reads.
#[tokio::test]
async fn a_queued_input_waits_past_the_start_deadline_for_its_own_turn() {
    // Arrange: a running turn, and an input waiting in the line behind it.
    let root = tempfile::tempdir().expect("root");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::clone(&store)));
    let target = target();
    let session: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&target).expect("target JSON"))
            .expect("hub session");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(Vec::new(), Arc::clone(&store)).expect("supervisor"),
    );
    let delivery = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::collections::HashSet::new(),
        collaboration_runtime::EndpointDirectory::new(target.endpoint.service_id.clone()),
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    ));
    let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let port = Arc::new(HostSessionCommandPort::new(
        Arc::clone(&supervisor),
        delivery,
        Arc::clone(&hub),
        store,
        Arc::clone(&clock) as Arc<dyn router_clock::RuntimeClock>,
    ));
    let input_id = InputId::generate();
    supervisor
        .queued_operation_registry()
        .record_queued_contents(
            OperationId::generate(),
            target.clone(),
            binding(&target),
            input_id.clone(),
            &[PromptContent::text("queued".to_owned()).expect("text")],
        );
    hub.publish(
        session.clone(),
        SessionEvent::TurnStarted {
            turn_id: "running".to_owned(),
            input_id: InputId::generate(),
        },
    )
    .await
    .expect("running turn");
    let waiting = {
        let (port, session, input_id) = (Arc::clone(&port), session.clone(), input_id.clone());
        tokio::spawn(async move { port.queue_outcome(session, input_id).await })
    };

    // The wait has started (and armed any timer it has) before time moves.
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }

    // Act: twice the start deadline passes while the running turn holds.
    clock
        .advance(std::time::Duration::from_secs(60))
        .expect("advance the injected clock");
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    let waited_past_the_deadline = !waiting.is_finished();
    for event in [
        SessionEvent::TurnEnded {
            turn_id: "running".to_owned(),
            outcome: TurnOutcome::Ended {
                stop_reason: StopReason::EndTurn,
                local_cause: None,
            },
        },
        SessionEvent::TurnStarted {
            turn_id: "its-own".to_owned(),
            input_id,
        },
    ] {
        hub.publish(session.clone(), event)
            .await
            .expect("turn event");
    }
    let outcome = waiting.await.expect("outcome task");

    // Assert
    assert!(
        waited_past_the_deadline,
        "still queued, not failed: {outcome:?}"
    );
    assert!(
        matches!(&outcome, Ok(QueuedInputOutcome::Started { turn_id }) if turn_id == "its-own"),
        "answered when its own turn started: {outcome:?}"
    );
}
