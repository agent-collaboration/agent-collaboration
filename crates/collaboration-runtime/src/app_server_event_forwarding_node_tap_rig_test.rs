//! N3 tap rig: drives the real `AppServerEventForwarding` with one session event, renders the
//! node's declared state, and records what N3 consumed and emitted.
use super::super::*;
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase, stable,
};
use session_event_model::{InputId, SessionItemKind, ToolCallStatus};

pub(crate) const RECEIPTS_EMPTY: &str = "outstanding=[] accepted_for_next_item=none";

pub(crate) struct Rig {
    pub(crate) forwarding: AppServerEventForwarding,
    pub(crate) session: SessionRef,
    pub(crate) thread_id: String,
}

pub(crate) fn tap_session() -> SessionRef {
    serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-7000-8000-000000000001",
        "endpointId":"claude-local"},"sessionId":"tap-session"
    }))
    .expect("session")
}

pub(crate) fn tap_actor() -> Identity {
    serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor")
}

impl Rig {
    pub(crate) fn new() -> Self {
        Self::with_broker(None)
    }

    pub(crate) fn with_broker(broker: Option<Arc<ServiceInteractionBroker>>) -> Self {
        let session = tap_session();
        Self {
            thread_id: thread_alias(&session),
            forwarding: AppServerEventForwarding::new(tap_actor(), broker),
            session,
        }
    }

    /// Drives the node without recording a tap (case setup).
    pub(crate) fn setup(&mut self, event: SessionEvent) -> Vec<Value> {
        self.forwarding
            .project(&self.session, &HubEvent { sequence: 1, event })
    }

    pub(crate) fn start_turn(&mut self, turn_id: &str) -> InputId {
        let input_id = InputId::generate();
        self.setup(SessionEvent::TurnStarted {
            turn_id: turn_id.into(),
            input_id: input_id.clone(),
        });
        input_id
    }

    pub(crate) fn register_user_input(&mut self, client_id: &str) -> InputId {
        let input_id = InputId::generate();
        self.forwarding.user_receipts.register(
            &json!({"params":{"threadId":self.thread_id,"clientUserMessageId":client_id}}),
            input_id.clone(),
        );
        input_id
    }

    pub(crate) fn snapshot(&self) -> NodeStateSnapshot {
        let session = &self.session;
        let mut stored = self
            .forwarding
            .items
            .iter()
            .filter(|((item_session, _), _)| item_session == session)
            .map(|((_, item_id), thread_item)| {
                let kind = thread_item["type"].as_str().unwrap_or("?");
                let text = match kind {
                    "agentMessage" => thread_item["text"].as_str().map(str::to_owned),
                    "reasoning" => thread_item["summary"][0].as_str().map(str::to_owned),
                    _ => None,
                };
                (item_id.clone(), kind.to_owned(), text)
            })
            .collect::<Vec<_>>();
        stored.sort();
        let stored_items =
            render_or_none(stored.iter().map(|(id, kind, _)| format!("{id}:{kind}")));
        let text_sent = render_or_none(
            stored
                .iter()
                .filter_map(|(id, _, text)| text.as_ref().map(|text| format!("{id}={text}"))),
        );
        let mut pending = self
            .forwarding
            .pending
            .values()
            .map(|interaction| match interaction {
                PendingAppServerInteraction::Approval { request, .. } => {
                    format!("approval:{}", request.request_id)
                }
                PendingAppServerInteraction::Question { request, .. } => {
                    format!("question:{}", request.request_id)
                }
            })
            .collect::<Vec<_>>();
        pending.sort();
        NodeStateSnapshot::new()
            .field(
                "active_turn",
                self.forwarding
                    .turn_ids
                    .get(session)
                    .map_or("none", String::as_str),
            )
            .field("stored_items", stored_items)
            .field("text_already_sent", text_sent)
            .field("pending_interactions", render_or_none(pending.into_iter()))
            .field(
                "working_directory",
                if self.forwarding.working_directories.contains_key(session) {
                    "registered"
                } else {
                    "none"
                },
            )
            .field(
                "receipts",
                crate::native_user_receipts::node_tap_tests::render_receipt_state(
                    &self.forwarding.user_receipts,
                    &self.thread_id,
                ),
            )
    }
}

fn render_or_none(parts: impl Iterator<Item = String>) -> String {
    let joined = parts.collect::<Vec<_>>().join(",");
    if joined.is_empty() {
        "none".into()
    } else {
        joined
    }
}

/// The state every case starts from; expectations override only what the row changes.
pub(crate) fn base_state() -> NodeStateSnapshot {
    NodeStateSnapshot::new()
        .field("active_turn", "none")
        .field("stored_items", "none")
        .field("text_already_sent", "none")
        .field("pending_interactions", "none")
        .field("working_directory", "none")
        .field("receipts", RECEIPTS_EMPTY)
}

pub(crate) fn item_kind_name(kind: &SessionItemKind) -> &'static str {
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

pub(crate) fn session_event_kind(event: &SessionEvent) -> String {
    match event {
        SessionEvent::TurnStarted { .. } => "TurnStarted".into(),
        SessionEvent::InputAccepted { .. } => "InputAccepted".into(),
        SessionEvent::TurnEnded { outcome, .. } => match outcome {
            TurnOutcome::Ended { stop_reason, .. } => format!("TurnEnded(Ended:{stop_reason:?})"),
            TurnOutcome::Lost { reason } => format!("TurnEnded(Lost:{reason:?})"),
        },
        SessionEvent::ItemStarted { item } => {
            format!("ItemStarted({})", item_kind_name(&item.kind))
        }
        SessionEvent::ItemUpdated { item } => {
            format!("ItemUpdated({})", item_kind_name(&item.kind))
        }
        SessionEvent::ItemCompleted { .. } => "ItemCompleted".into(),
        SessionEvent::InteractionRequested { .. } => "InteractionRequested".into(),
        SessionEvent::InteractionResolved { .. } => "InteractionResolved".into(),
        SessionEvent::StateChanged { .. } => "StateChanged".into(),
        SessionEvent::CapabilitiesChanged { .. } => "CapabilitiesChanged".into(),
        SessionEvent::SettingsChanged { .. } => "SettingsChanged".into(),
        SessionEvent::ResyncRequired { .. } => "ResyncRequired".into(),
    }
}

/// The thread alias of [`tap_session`], pinned: a UUIDv5 over the fixed machine and endpoint.
pub(crate) const THREAD_ID: &str = "91ae3711-254d-5b31-a1d6-88a631add296";

/// One expected frame: its method and params, with the thread id filled in.
pub(crate) fn frame(method: &str, params: Value) -> Value {
    let mut params = params;
    params["threadId"] = json!(THREAD_ID);
    json!({"method":method,"params":params})
}

pub(crate) fn turn_frame(method: &str, turn_id: &str, status: &str, error: Value) -> Value {
    frame(
        method,
        json!({"turn":{
            "id":turn_id,"items":[],"status":status,"error":error,
            "startedAt":null,"completedAt":null,"durationMs":null
        }}),
    )
}

pub(crate) fn item_started_frame(turn_id: &str, item: Value) -> Value {
    frame(
        "item/started",
        json!({"turnId":turn_id,"item":item,"startedAtMs":"<volatile:startedAtMs>"}),
    )
}

pub(crate) fn item_completed_frame(turn_id: &str, item: Value) -> Value {
    frame(
        "item/completed",
        json!({"turnId":turn_id,"item":item,"completedAtMs":"<volatile:completedAtMs>"}),
    )
}

pub(crate) fn text_delta_frame(method: &str, turn_id: &str, item_id: &str, delta: &str) -> Value {
    let mut params = json!({"turnId":turn_id,"itemId":item_id,"delta":delta});
    if method == "item/reasoning/summaryTextDelta" {
        params["summaryIndex"] = json!(0);
    }
    frame(method, params)
}

pub(crate) fn expectation(
    frames: &[Value],
    state_after: NodeStateSnapshot,
) -> CatalogRowExpectation {
    let mut row = CatalogRowExpectation::new();
    for expected in frames {
        row = row.produces_with(
            MessageFamily::CodexMessageToCli,
            EdgeParty::Node(ProtocolNode::N1CliFace),
            expected["method"].as_str().unwrap_or("<no method>"),
            expected.clone(),
        );
    }
    row.state_after(state_after)
}

/// The consumed event's decoded content; the generated input id is made stable.
pub(crate) fn event_payload(event: &SessionEvent) -> Value {
    stable(
        serde_json::to_value(event).expect("encode session event"),
        &["inputId"],
    )
}

/// The clock reads N3 stamps on a frame: replaced in the compared copy, kept in the raw frame.
pub(crate) const CLOCK_FIELDS: &[&str] = &["startedAtMs", "completedAtMs"];

/// Records every frame `forwarding.project()` returned, the value N1 would send to the CLI; the
/// compared copy replaces the fields named in `volatile`. These are a Rust API result
/// (`TypedOutcome`), never frames on a connection: only N1's connection-loop cases record the
/// CLI wire.
pub(crate) fn record_frames(case: &mut TapCase, frames: &[Value], volatile: &[&str]) {
    for frame in frames {
        case.produced_raw(
            RecordLayer::TypedOutcome,
            MessageFamily::CodexMessageToCli,
            EdgeParty::Node(ProtocolNode::N1CliFace),
            frame["method"].as_str().unwrap_or("<no method>"),
            frame.clone(),
            volatile,
        );
    }
}

/// Feeds one event, records what N3 consumed and emitted, and compares with the row.
pub(crate) fn tap_event(
    rig: &mut Rig,
    case_name: &str,
    catalog_row: &str,
    event: SessionEvent,
    row: &CatalogRowExpectation,
) -> Vec<Value> {
    tap_event_normalized(
        rig,
        case_name,
        catalog_row,
        event,
        row,
        |_| {},
        CLOCK_FIELDS,
    )
}

/// [`tap_event`] where `order` first puts frames whose order is not fixed (hash-map iteration)
/// into a canonical order, and `volatile` names the generated fields the compared copy replaces.
/// Each frame is still recorded exactly as N3 emitted it.
pub(crate) fn tap_event_normalized(
    rig: &mut Rig,
    case_name: &str,
    catalog_row: &str,
    event: SessionEvent,
    row: &CatalogRowExpectation,
    order: impl FnOnce(&mut Vec<Value>),
    volatile: &[&str],
) -> Vec<Value> {
    let mut case = TapCase::new(ProtocolNode::N3OutputMapper, case_name, catalog_row);
    case.state_before(rig.snapshot());
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N5EventHub),
        &session_event_kind(&event),
        event_payload(&event),
    );
    let frames = rig.setup(event);
    let mut recorded = frames.clone();
    order(&mut recorded);
    record_frames(&mut case, &recorded, volatile);
    case.state_after(rig.snapshot());
    case.finish(row).assert_settled();
    frames
}

pub(crate) fn item(item_id: &str, kind: SessionItemKind, text: Option<&str>) -> SessionItem {
    SessionItem {
        item_id: item_id.into(),
        kind,
        text: text.map(str::to_owned),
    }
}

pub(crate) fn agent_message(text: &str) -> SessionItem {
    item("m", SessionItemKind::AgentMessage, Some(text))
}

pub(crate) fn thought(text: &str) -> SessionItem {
    item("t", SessionItemKind::AgentThought, Some(text))
}

pub(crate) fn tool(status: ToolCallStatus, text: Option<&str>) -> SessionItem {
    item(
        "tool",
        SessionItemKind::ToolCall {
            tool_kind: "read".into(),
            status,
        },
        text,
    )
}

pub(crate) fn started(item: SessionItem) -> SessionEvent {
    SessionEvent::ItemStarted { item }
}

pub(crate) fn updated(item: SessionItem) -> SessionEvent {
    SessionEvent::ItemUpdated { item }
}
