//! A queued input's outcome (D1 R12, review F2): waiting in the session's
//! line is not a dispatch that failed to start, so no start deadline applies.
//! Time moves only on the port's one injected clock (review C1).

use super::HostSessionCommandPort;
use crate::{
    ExternalProviderSupervisor, LiveSessionOwnership, LiveSessionOwnershipCheck,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::{DeliveryFuture, QueuedInputOutcome, SessionCommandPort as _};
use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, NonEmptyText, OperationId,
    ProviderBindingId, ProviderBindingIdentity, ProviderKind, ProviderRuntimeIdentity,
    ProviderTransport, SessionId, SessionRef,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use session_event_model::{InputId, PromptContent, SessionEvent, StopReason, TurnOutcome};
use std::sync::Arc;
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
