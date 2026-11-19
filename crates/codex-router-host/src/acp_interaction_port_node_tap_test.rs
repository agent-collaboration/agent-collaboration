//! N7 node taps: an approval request in from the ACP client (N6), interaction
//! events out to the event hub (N5), and the decision back out to N6.
//!
//! Each case drives the real `HostInteractionPort` against the real typed
//! `ServiceInteractionBroker`. Interaction events are read from a recording
//! event sink, the option handed back to the agent from the port's own outcome,
//! and pending-approval state from the broker's list.

use super::*;
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, HarnessSessionRef,
    ProviderIdentity, SessionId,
};
use collaboration_runtime::{NativeControlBackend, NativeGenerationGate, TypedInteractionDecision};
use tokio::sync::mpsc;

const SERVICE_ID: &str = "00000000-0000-7000-8000-000000000001";

struct ChannelEventSink(mpsc::UnboundedSender<SessionEvent>);

impl SessionEventSink for ChannelEventSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _: &str) -> acp_client_runtime::HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(
        &self,
        _: &str,
        event: SessionEvent,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        self.0
            .send(event)
            .map_err(|_| acp_client_runtime::EventSinkClosed)
    }
}

struct BrokerRig {
    _root: tempfile::TempDir,
    broker: Arc<ServiceInteractionBroker>,
    port: Arc<HostInteractionPort>,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    target: HarnessSessionRef,
    requester: HarnessSessionRef,
    approver: ProviderIdentity,
    actor: Identity,
}

async fn broker_rig() -> BrokerRig {
    let root = tempfile::tempdir().expect("rig root");
    let service_id: workspace_ids::MachineId =
        SERVICE_ID.to_owned().try_into().expect("service ID");
    let provider_endpoint = EndpointRef {
        service_id: service_id.clone(),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let approver_endpoint = EndpointRef {
        service_id: service_id.clone(),
        endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("endpoint"),
    };
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
            .await
            .expect("interaction store"),
    );
    let broker = ServiceInteractionBroker::load(
        service_id,
        NativeControlBackend {
            codex_home: root.path().to_owned(),
            endpoint: approver_endpoint.clone(),
            gate: NativeGenerationGate::default(),
        },
        interaction_store,
    )
    .await
    .expect("broker");
    let (sender, events) = mpsc::unbounded_channel();
    let port = Arc::new(HostInteractionPort::default());
    port.install_broker(Arc::clone(&broker)).await;
    port.install_event_sink(Arc::new(ChannelEventSink(sender)) as Arc<dyn SessionEventSink>)
        .await;
    let approver = ProviderIdentity::Human {
        human_id: "owner".to_owned().try_into().expect("human ID"),
    };
    let actor = approver.to_board_identity().expect("board actor");
    BrokerRig {
        _root: root,
        broker,
        port,
        events,
        target: HarnessSessionRef {
            endpoint: provider_endpoint,
            session_id: SessionId::try_from("session-one".to_owned()).expect("session"),
        },
        requester: HarnessSessionRef {
            endpoint: approver_endpoint,
            session_id: SessionId::try_from("requester".to_owned()).expect("requester"),
        },
        approver,
        actor,
    }
}

impl BrokerRig {
    fn context(&self) -> ExternalProviderApprovalContext {
        ExternalProviderApprovalContext {
            requester: self.requester.clone().into(),
            approver: self.approver.clone(),
            target: self.target.clone(),
            operation_id: OperationId::generate(),
            binding_generation: CodexGeneration {
                service_epoch: collaboration_protocol::UuidIdentity::try_from(
                    SERVICE_ID.to_owned(),
                )
                .expect("epoch"),
                generation: GenerationNumber::try_from(1).expect("generation"),
            },
            binding_retirement: CancellationToken::new(),
        }
    }

    async fn pending_approvals(&self) -> usize {
        self.broker.list(true).await.approvals.len()
    }

    /// Starts the agent's approval request and waits, event by event, until N7
    /// has published the pending interaction to N5.
    async fn request_and_wait_until_pending(
        &mut self,
        request_id: &str,
    ) -> (
        tokio::task::JoinHandle<ApprovalPortOutcome>,
        Vec<SessionEvent>,
    ) {
        use acp_client_runtime::InteractionPort as _;
        let request = approval_request(request_id);
        let port = Arc::clone(&self.port);
        let context = self.context();
        let task = tokio::spawn(async move {
            port.request_approval(
                context,
                request,
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await
        });
        let pending = self.events.recv().await.expect("pending interaction event");
        (task, vec![pending])
    }
}

/// The approval request the agent offers: one allow-once option.
fn approval_request(request_id: &str) -> ApprovalRequest {
    serde_json::from_value(serde_json::json!({
        "requestId": request_id, "title": "Run command", "options": [
            {"optionId":"allow-once","label":"Allow once",
             "choice":{"effect":"allow","scope":"once"}}
        ]
    }))
    .expect("approval request")
}

fn event_kind(event: &SessionEvent) -> String {
    match event {
        SessionEvent::InteractionRequested {
            interaction: PendingInteraction::Approval { .. },
        } => "InteractionRequested".to_owned(),
        SessionEvent::InteractionResolved { .. } => "InteractionResolved".to_owned(),
        other => format!("unexpected:{other:?}"),
    }
}

fn outcome_kind(outcome: &ApprovalPortOutcome) -> String {
    match outcome {
        ApprovalPortOutcome::Selected { option_id, .. } => format!("selected:{option_id}"),
        ApprovalPortOutcome::Cancelled => "cancelled".to_owned(),
        ApprovalPortOutcome::Unavailable => "unavailable".to_owned(),
    }
}

fn record_events_to_n5(case: &mut TapCase, events: &[SessionEvent]) {
    for event in events {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N5EventHub),
            &event_kind(event),
            serde_json::to_value(event).expect("event JSON"),
        );
    }
}

/// The whole reply N7 hands back to the agent, as its full `Debug` text (the outcome has no serde
/// form).
fn outcome_payload(outcome: &ApprovalPortOutcome) -> serde_json::Value {
    serde_json::json!({"debug": format!("{outcome:#?}")})
}

/// Everything the hub has received beyond what was already read.
fn drain_events(events: &mut mpsc::UnboundedReceiver<SessionEvent>) -> Vec<SessionEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

/// The catalog row for one settled request: the pending interaction and its resolution to N5, then
/// the decision to N6, each with the values written here, and no approval left pending.
fn settled_row(
    request_id: &str,
    decision_kind: &str,
    decision: &ApprovalPortOutcome,
) -> CatalogRowExpectation {
    CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N5EventHub),
            "InteractionRequested",
            serde_json::json!({
                "kind": "interactionRequested",
                "interaction": {
                    "kind": "approval",
                    "approver": {"kind": "human", "humanId": "owner"},
                    "request": {
                        "requestId": request_id,
                        "title": "Run command",
                        "optionsOrigin": "agentOffered",
                        "options": [{
                            "optionId": "allow-once",
                            "label": "Allow once",
                            "choice": {"effect": "allow", "scope": "once"}
                        }]
                    }
                }
            }),
        )
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N5EventHub),
            "InteractionResolved",
            serde_json::json!({"kind": "interactionResolved", "requestId": request_id}),
        )
        .produces_with(
            MessageFamily::ApprovalDecision,
            EdgeParty::Node(ProtocolNode::N6AcpClient),
            decision_kind,
            outcome_payload(decision),
        )
        .state_after(pending_state(0))
}

fn pending_state(count: usize) -> NodeStateSnapshot {
    NodeStateSnapshot::new().field("pending_approvals", count)
}

/// A decision N7 took in, as the whole typed value's `Debug` text (the value has no serde form).
fn consumed_decision(
    case: &mut TapCase,
    from: ProtocolNode,
    kind: &str,
    decision: &TypedInteractionDecision,
) {
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::ApprovalDecision,
        EdgeParty::Node(from),
        kind,
        serde_json::json!({"debug": format!("{decision:#?}")}),
    );
}

fn consumed_request(case: &mut TapCase, request_id: &str) {
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::ApprovalRequest,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        "session/request_permission",
        serde_json::to_value(approval_request(request_id)).expect("request JSON"),
    );
}

#[tokio::test]
async fn node_tap_approval_selected_by_the_approver_reaches_the_agent_once() {
    let mut rig = broker_rig().await;
    let mut case = TapCase::new(
        ProtocolNode::N7ApprovalsBroker,
        "approval_selected_option_returns_to_agent",
        "N7: approval request in from N6; interaction events to N5; decision in; selected option out to N6",
    );
    case.state_before(pending_state(rig.pending_approvals().await));
    consumed_request(&mut case, "approval-one");
    let (task, mut events) = rig.request_and_wait_until_pending("approval-one").await;
    let decision = TypedInteractionDecision::SelectApproval {
        option_id: "allow-once".into(),
        acknowledge_persistent: false,
        note: None,
    };
    consumed_decision(
        &mut case,
        ProtocolNode::N1CliFace,
        "SelectApproval(allow-once)",
        &decision,
    );
    rig.broker
        .decide_typed_interaction("approval-one", &rig.actor, decision)
        .await
        .expect("approver decision");
    let outcome = task.await.expect("port task");
    events.extend(drain_events(&mut rig.events));
    record_events_to_n5(&mut case, &events);
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::ApprovalDecision,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        &outcome_kind(&outcome),
        outcome_payload(&outcome),
    );
    case.state_after(pending_state(rig.pending_approvals().await));
    let row = settled_row(
        "approval-one",
        "selected:allow-once",
        &ApprovalPortOutcome::Selected {
            option_id: "allow-once".into(),
            note: None,
        },
    );
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_first_decision_wins_and_a_second_decision_does_not_reach_the_agent() {
    let mut rig = broker_rig().await;
    let mut case = TapCase::new(
        ProtocolNode::N7ApprovalsBroker,
        "first_decision_wins",
        "N7: the broker settles each request once; the first decision wins",
    );
    case.state_before(pending_state(rig.pending_approvals().await));
    consumed_request(&mut case, "approval-race");
    let (task, mut events) = rig.request_and_wait_until_pending("approval-race").await;
    let select = || TypedInteractionDecision::SelectApproval {
        option_id: "allow-once".into(),
        acknowledge_persistent: false,
        note: None,
    };
    consumed_decision(
        &mut case,
        ProtocolNode::N1CliFace,
        "SelectApproval(allow-once)",
        &select(),
    );
    rig.broker
        .decide_typed_interaction("approval-race", &rig.actor, select())
        .await
        .expect("first decision");
    consumed_decision(
        &mut case,
        ProtocolNode::N11McpServer,
        "Cancel",
        &TypedInteractionDecision::Cancel,
    );
    let second = rig
        .broker
        .decide_typed_interaction(
            "approval-race",
            &rig.actor,
            TypedInteractionDecision::Cancel,
        )
        .await;
    assert!(second.is_err(), "the second decision must be refused");
    let outcome = task.await.expect("port task");
    // The settled request leaves nothing further to deliver to the agent or the hub: any extra
    // event drained here is an extra record and fails the comparison.
    events.extend(drain_events(&mut rig.events));
    record_events_to_n5(&mut case, &events);
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::ApprovalDecision,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        &outcome_kind(&outcome),
        outcome_payload(&outcome),
    );
    case.state_after(pending_state(rig.pending_approvals().await));
    let row = settled_row(
        "approval-race",
        "selected:allow-once",
        &ApprovalPortOutcome::Selected {
            option_id: "allow-once".into(),
            note: None,
        },
    );
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_approval_cancelled_by_the_approver_returns_cancelled_to_the_agent() {
    let mut rig = broker_rig().await;
    let mut case = TapCase::new(
        ProtocolNode::N7ApprovalsBroker,
        "approval_cancelled_returns_to_agent",
        "N7: decision in (cancel); cancelled out to N6; interaction events to N5",
    );
    case.state_before(pending_state(rig.pending_approvals().await));
    consumed_request(&mut case, "approval-cancel");
    let (task, mut events) = rig.request_and_wait_until_pending("approval-cancel").await;
    consumed_decision(
        &mut case,
        ProtocolNode::N11McpServer,
        "Cancel",
        &TypedInteractionDecision::Cancel,
    );
    rig.broker
        .decide_typed_interaction(
            "approval-cancel",
            &rig.actor,
            TypedInteractionDecision::Cancel,
        )
        .await
        .expect("approver cancel");
    let outcome = task.await.expect("port task");
    events.extend(drain_events(&mut rig.events));
    record_events_to_n5(&mut case, &events);
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::ApprovalDecision,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        &outcome_kind(&outcome),
        outcome_payload(&outcome),
    );
    case.state_after(pending_state(rig.pending_approvals().await));
    let row = settled_row(
        "approval-cancel",
        "cancelled",
        &ApprovalPortOutcome::Cancelled,
    );
    case.finish(&row).assert_settled();
}
