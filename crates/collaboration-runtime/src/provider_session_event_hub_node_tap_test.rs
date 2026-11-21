//! N5 node taps: the real provider Session event hub, one published event at a
//! time, observed at every subscriber kind that exists in this crate.
//!
//! N3 (output mapper), N11 (`events_observe`) and N4 (waiting for `TurnStarted`)
//! all reach the hub through `attach`; inventory reads it through `sessions`.
use std::sync::Arc;

use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase, stable,
};
use collaboration_contracts::{HubEvent, HubSessionSummary, SessionEventHub};
use message_board::SessionRef;
use session_event_model::{
    ApprovalChoice, ApprovalEffect, ApprovalRequest, ApprovalScope, OfferedOption, OfferedOptionId,
    OfferedOptions, OptionsOrigin, PendingInteraction, SessionEvent, SessionItem, SessionItemKind,
    SessionState,
};
use tokio::sync::{Mutex, broadcast};

use super::ProviderSessionEventHub;
use crate::ProviderOperationStore;

/// The inventory summary's timestamp is a clock read, so only it is replaced before recording.
const VOLATILE_UPDATED_AT_SECONDS: i64 = -1;

const N5: ProtocolNode = ProtocolNode::N5EventHub;
const SUBSCRIBERS: [(&str, EdgeParty); 3] = [
    ("N3", EdgeParty::Node(ProtocolNode::N3OutputMapper)),
    ("N11", EdgeParty::Node(ProtocolNode::N11McpServer)),
    ("N4", EdgeParty::Node(ProtocolNode::N4CommandPort)),
];

struct HubFixture {
    hub: ProviderSessionEventHub,
    session: SessionRef,
    _root: tempfile::TempDir,
}

async fn fixture() -> HubFixture {
    let root = tempfile::tempdir().expect("root");
    let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    let session: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":uuid::Uuid::now_v7(),"endpointId":"claude-local"},
        "sessionId":uuid::Uuid::now_v7()
    }))
    .expect("session");
    let owner: collaboration_protocol::ProviderIdentity =
        serde_json::from_value(serde_json::json!({"humanId":"owner"})).expect("owner");
    store
        .record_session(&crate::ProviderSessionRecord {
            target: serde_json::from_value(serde_json::to_value(&session).expect("target"))
                .expect("target"),
            working_directory: root.path().display().to_string().try_into().expect("cwd"),
            requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                access: collaboration_protocol::RouterAccess::WriteRestricted,
            },
            created_by: owner.clone(),
            approver: owner,
            updated_at_ms: 1,
            resume: Default::default(),
        })
        .await
        .expect("inventory");
    HubFixture {
        hub: ProviderSessionEventHub::new(Arc::new(Mutex::new(store))),
        session,
        _root: root,
    }
}

fn variant_name(kind: &SessionItemKind) -> &'static str {
    match kind {
        SessionItemKind::UserMessage => "UserMessage",
        SessionItemKind::AgentMessage => "AgentMessage",
        SessionItemKind::AgentThought => "AgentThought",
        SessionItemKind::ToolCall { .. } => "ToolCall",
        SessionItemKind::Plan => "Plan",
        SessionItemKind::Usage => "Usage",
        SessionItemKind::ModeChange => "ModeChange",
        SessionItemKind::ConfigChange => "ConfigChange",
        SessionItemKind::SessionInfo => "SessionInfo",
        SessionItemKind::Notice => "Notice",
        SessionItemKind::Unknown { .. } => "Unknown",
    }
}

/// The kind string of a hub event; metadata items show whether their payload
/// is a JSON string, which is today's defect on the wire.
fn event_kind(event: &SessionEvent) -> String {
    match event {
        SessionEvent::TurnStarted { .. } => "TurnStarted".into(),
        SessionEvent::TurnEnded { .. } => "TurnEnded".into(),
        SessionEvent::ItemStarted { item } => format!("ItemStarted({})", item_label(item)),
        SessionEvent::ItemUpdated { item } => format!("ItemUpdated({})", item_label(item)),
        SessionEvent::ItemCompleted { .. } => "ItemCompleted".into(),
        SessionEvent::InputAccepted { .. } => "InputAccepted".into(),
        SessionEvent::InteractionRequested { .. } => "InteractionRequested".into(),
        SessionEvent::InteractionResolved { .. } => "InteractionResolved".into(),
        SessionEvent::SettingsChanged { .. } => "SettingsChanged".into(),
        SessionEvent::StateChanged { .. } => "StateChanged".into(),
        SessionEvent::CapabilitiesChanged { .. } => "CapabilitiesChanged".into(),
        SessionEvent::ResyncRequired { .. } => "ResyncRequired".into(),
    }
}

fn item_label(item: &SessionItem) -> String {
    let name = variant_name(&item.kind);
    match &item.kind {
        SessionItemKind::Usage
        | SessionItemKind::ConfigChange
        | SessionItemKind::SessionInfo
        | SessionItemKind::Notice => {
            let text = item.text.as_deref().unwrap_or_default();
            let payload = if serde_json::from_str::<serde_json::Value>(text)
                .is_ok_and(|value| value.is_object() || value.is_array())
            {
                "jsonText"
            } else {
                "plainText"
            };
            format!("{name} payload={payload}")
        }
        _ => name.to_owned(),
    }
}

fn hub_state(state: &SessionState) -> &'static str {
    match state {
        SessionState::Unloaded => "Unloaded",
        SessionState::Idle => "Idle",
        SessionState::Running => "Running",
        SessionState::RequiresAction { .. } => "RequiresAction",
        SessionState::AuthenticationRequired => "AuthenticationRequired",
        SessionState::Closed => "Closed",
    }
}

async fn hub_snapshot_state(fixture: &HubFixture) -> NodeStateSnapshot {
    let state = fixture
        .hub
        .state(fixture.session.clone())
        .await
        .map_or("unknown", |state| hub_state(&state));
    let retained = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .map_or(0, |attachment| attachment.snapshot.len());
    NodeStateSnapshot::new()
        .field("session_state", state)
        .field("retained_events", retained)
}

struct Subscriber {
    name: &'static str,
    party: EdgeParty,
    receiver: broadcast::Receiver<HubEvent>,
}

async fn attach_subscribers(fixture: &HubFixture) -> Vec<Subscriber> {
    let mut subscribers = Vec::new();
    for (name, party) in SUBSCRIBERS {
        let attachment = fixture
            .hub
            .attach(fixture.session.clone())
            .await
            .expect("attach");
        subscribers.push(Subscriber {
            name,
            party,
            receiver: attachment.receiver,
        });
    }
    subscribers
}

/// The decoded event as it travelled; generated input ids are made stable.
fn event_payload(event: &SessionEvent) -> serde_json::Value {
    stable(
        serde_json::to_value(event).expect("encode session event"),
        &["inputId"],
    )
}

/// The whole hub event as delivered: its sequence number and the whole session event.
fn hub_event_payload(delivered: &HubEvent) -> serde_json::Value {
    serde_json::json!({
        "sequence": delivered.sequence,
        "event": event_payload(&delivered.event),
    })
}

/// Records every event each subscriber already holds, draining its receiver until empty.
/// `publish` hands the event to the broadcast before it returns, so an event that is not there now
/// was never delivered, and an extra (duplicated or undeclared) delivery becomes an extra record.
fn record_deliveries(case: &mut TapCase, subscribers: &mut [Subscriber]) {
    for subscriber in subscribers {
        loop {
            match subscriber.receiver.try_recv() {
                Ok(delivered) => case.produced_with(
                    RecordLayer::TypedOutcome,
                    MessageFamily::SessionEvent,
                    subscriber.party,
                    &event_kind(&delivered.event),
                    hub_event_payload(&delivered),
                ),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(error) => panic!("{} received {error}", subscriber.name),
            }
        }
    }
}

/// Every subscriber receives the same whole hub event: `sequence` and the session event.
fn sequential_row(kind: &str, sequence: u64, event: &serde_json::Value) -> CatalogRowExpectation {
    let payload = serde_json::json!({"sequence": sequence, "event": event});
    SUBSCRIBERS
        .iter()
        .fold(CatalogRowExpectation::new(), |row, (_, party)| {
            row.produces_with(MessageFamily::SessionEvent, *party, kind, payload.clone())
        })
}

#[tokio::test]
async fn node_tap_n5_turn_started_reaches_every_subscriber() {
    // Arrange: N3, N11 and N4 attached, inventory reading the same hub.
    let fixture = fixture().await;
    fixture
        .hub
        .publish(
            fixture.session.clone(),
            SessionEvent::StateChanged {
                state: SessionState::Idle,
            },
        )
        .await
        .expect("idle");
    let mut subscribers = attach_subscribers(&fixture).await;
    let mut case = TapCase::new(
        N5,
        "turn_started_fan_out",
        "N5 node: events reach N3, N11, inventory and N4",
    );
    case.state_before(hub_snapshot_state(&fixture).await);
    let turn_started =
        serde_json::json!({"kind":"turnStarted","turnId":"turn-1","inputId":"<volatile:inputId>"});
    let published = SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id: session_event_model::InputId::generate(),
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        "TurnStarted",
        event_payload(&published),
    );
    // Act
    fixture
        .hub
        .publish(fixture.session.clone(), published)
        .await
        .expect("publish");
    record_deliveries(&mut case, &mut subscribers);
    // Inventory reads the hub's projection: Running means it saw the event.
    let inventory = fixture
        .hub
        .sessions(fixture.session.endpoint.clone())
        .await
        .expect("sessions")
        .remove(0);
    // The whole summary as `Debug` text (it has no serde); `updated_at_seconds` is a clock read.
    let mut inventory = inventory;
    inventory.updated_at_seconds = VOLATILE_UPDATED_AT_SECONDS;
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::SessionEvent,
        EdgeParty::Inventory,
        "TurnStarted",
        serde_json::json!({"debug": format!("{inventory:#?}")}),
    );
    case.state_after(hub_snapshot_state(&fixture).await);
    // Assert
    let expected_summary = HubSessionSummary {
        session: fixture.session.clone(),
        approver: serde_json::from_value(serde_json::json!({"kind":"human","humanId":"owner"}))
            .expect("approver"),
        working_directory: fixture._root.path().to_path_buf(),
        updated_at_seconds: VOLATILE_UPDATED_AT_SECONDS,
        preview: String::new(),
        name: None,
        model: None,
        mode: None,
        state: SessionState::Running,
    };
    let row = sequential_row("TurnStarted", 2, &turn_started)
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Inventory,
            "TurnStarted",
            serde_json::json!({"debug": format!("{expected_summary:#?}")}),
        )
        .state_after(
            NodeStateSnapshot::new()
                .field("session_state", "Running")
                .field("retained_events", 2),
        );
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n5_interaction_event_from_n7_reaches_subscribers() {
    // Arrange
    let fixture = fixture().await;
    let mut subscribers = attach_subscribers(&fixture).await;
    let approver: message_board::Identity =
        serde_json::from_value(serde_json::json!({"kind":"human","humanId":"owner"}))
            .expect("approver");
    let request = ApprovalRequest {
        request_id: "approval-1".into(),
        title: "Run pwd".into(),
        description: None,
        subject: None,
        options_origin: OptionsOrigin::AgentOffered,
        options: OfferedOptions::new(vec![OfferedOption {
            option_id: OfferedOptionId::new("allow-once").expect("option id"),
            label: "Allow once".into(),
            choice: ApprovalChoice::new(ApprovalEffect::Allow, ApprovalScope::Once),
        }])
        .expect("options"),
    };
    let mut case = TapCase::new(
        N5,
        "interaction_requested_from_n7",
        "N5 node: N7 interaction events -> hub -> subscribers",
    );
    case.state_before(hub_snapshot_state(&fixture).await);
    let interaction_requested = serde_json::json!({
        "kind":"interactionRequested",
        "interaction":{
            "kind":"approval",
            "approver":{"kind":"human","humanId":"owner"},
            "request":{
                "requestId":"approval-1",
                "title":"Run pwd",
                "optionsOrigin":"agentOffered",
                "options":[{"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}]
            }
        }
    });
    let published = SessionEvent::InteractionRequested {
        interaction: PendingInteraction::Approval {
            approver,
            request: Box::new(request),
        },
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N7ApprovalsBroker),
        "InteractionRequested",
        event_payload(&published),
    );
    // Act
    fixture
        .hub
        .publish(fixture.session.clone(), published)
        .await
        .expect("publish");
    record_deliveries(&mut case, &mut subscribers);
    case.state_after(hub_snapshot_state(&fixture).await);
    // Assert
    let row = sequential_row("InteractionRequested", 1, &interaction_requested).state_after(
        NodeStateSnapshot::new()
            .field("session_state", "RequiresAction")
            .field("retained_events", 1),
    );
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n5_late_subscriber_replays_history_in_order() {
    // Arrange: two events already published before anyone attaches.
    let fixture = fixture().await;
    for event in [
        SessionEvent::TurnStarted {
            turn_id: "turn-1".into(),
            input_id: session_event_model::InputId::generate(),
        },
        SessionEvent::StateChanged {
            state: SessionState::Running,
        },
    ] {
        fixture
            .hub
            .publish(fixture.session.clone(), event)
            .await
            .expect("publish");
    }
    let mut case = TapCase::new(
        N5,
        "late_subscriber_replay",
        "N5 node: broadcast + history; attach replays the snapshot",
    );
    case.state_before(hub_snapshot_state(&fixture).await);
    // Act: N3 attaches late and reads the snapshot.
    let attachment = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    for event in &attachment.snapshot {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N3OutputMapper),
            &event_kind(&event.event),
            hub_event_payload(event),
        );
    }
    case.state_after(hub_snapshot_state(&fixture).await);
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N3OutputMapper),
            "TurnStarted",
            serde_json::json!({"sequence":1,"event":{"kind":"turnStarted","turnId":"turn-1","inputId":"<volatile:inputId>"}}),
        )
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N3OutputMapper),
            "StateChanged",
            serde_json::json!({"sequence":2,"event":{"kind":"stateChanged","state":{"kind":"running"}}}),
        )
        .state_after(
            NodeStateSnapshot::new()
                .field("session_state", "Running")
                .field("retained_events", 2),
        );
    case.finish(&row).assert_settled();
}

/// Publishes one metadata item whose text is a JSON string, as N6 does today,
/// and records what N3 receives.
async fn tap_metadata_payload(
    case_name: &str,
    row_name: &str,
    kind: SessionItemKind,
    variant: &str,
    variant_json_kind: &str,
    json_text: &str,
    typed_payload: serde_json::Value,
) {
    // Arrange
    let fixture = fixture().await;
    let mut subscribers = attach_subscribers(&fixture).await;
    subscribers.truncate(1);
    let mut case = TapCase::new(N5, case_name, row_name);
    case.state_before(hub_snapshot_state(&fixture).await);
    let variant_kind = serde_json::json!({"kind": variant_json_kind});
    let started_with_json_text = serde_json::json!({
        "kind":"itemStarted",
        "item":{"itemId":"item-1","kind":variant_kind,"text":json_text}
    });
    let published = SessionEvent::ItemStarted {
        item: SessionItem {
            item_id: "item-1".to_owned(),
            kind,
            text: Some(json_text.to_owned()),
        },
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N6AcpClient),
        &format!("ItemStarted({variant} payload=jsonText)"),
        event_payload(&published),
    );
    // Act
    fixture
        .hub
        .publish(fixture.session.clone(), published)
        .await
        .expect("publish");
    record_deliveries(&mut case, &mut subscribers);
    let after = hub_snapshot_state(&fixture).await;
    case.state_after(after.clone());
    // Assert: today the hub carries the JSON string; the catalog wants a typed payload.
    case.current_defect(
        CatalogRowExpectation::new()
            .produces_with(
                MessageFamily::SessionEvent,
                EdgeParty::Node(ProtocolNode::N3OutputMapper),
                &format!("ItemStarted({variant} payload=jsonText)"),
                serde_json::json!({"sequence":1,"event":started_with_json_text}),
            )
            .state_after(after.clone()),
    );
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionEvent,
            EdgeParty::Node(ProtocolNode::N3OutputMapper),
            &format!("ItemStarted({variant} payload=typed)"),
            serde_json::json!({"sequence":1,"event":{
                "kind":"itemStarted",
                "item":{"itemId":"item-1","kind":variant_kind,"payload":typed_payload}
            }}),
        )
        .state_after(after);
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n5_usage_arrives_as_json_text() {
    tap_metadata_payload(
        "usage_payload_json_text",
        "N5 node: usage arrives as a JSON string inside SessionItem.text (D2)",
        SessionItemKind::Usage,
        "Usage",
        "usage",
        r#"{"used":120,"size":1000}"#,
        serde_json::json!({"used":120,"size":1000}),
    )
    .await;
}

#[tokio::test]
async fn node_tap_n5_config_options_arrive_as_json_text() {
    tap_metadata_payload(
        "config_options_payload_json_text",
        "N5 node: config options arrive as a JSON string inside SessionItem.text (D2)",
        SessionItemKind::ConfigChange,
        "ConfigChange",
        "configChange",
        r#"{"configOptions":[{"id":"model","name":"Model"}]}"#,
        serde_json::json!({"configOptions":[{"id":"model","name":"Model"}]}),
    )
    .await;
}

#[tokio::test]
async fn node_tap_n5_session_info_arrives_as_json_text() {
    tap_metadata_payload(
        "session_info_payload_json_text",
        "N5 node: session info arrives as a JSON string inside SessionItem.text (D2)",
        SessionItemKind::SessionInfo,
        "SessionInfo",
        "sessionInfo",
        r#"{"title":"Provider title"}"#,
        serde_json::json!({"title":"Provider title"}),
    )
    .await;
}

#[tokio::test]
async fn node_tap_n5_available_commands_arrive_as_json_text() {
    tap_metadata_payload(
        "available_commands_payload_json_text",
        "N5 node: available commands arrive as a JSON string inside SessionItem.text (D2)",
        SessionItemKind::Notice,
        "Notice",
        "notice",
        r#"{"availableCommands":[{"name":"compact","description":"Compact"}]}"#,
        serde_json::json!({"availableCommands":[{"name":"compact","description":"Compact"}]}),
    )
    .await;
}
