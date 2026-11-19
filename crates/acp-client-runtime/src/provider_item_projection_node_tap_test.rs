//! N6 tap cases for the ACP session update to session event conversion.
//!
//! One catalog input per case. The recording sink at the projection's outbound
//! edge is the only source of `produced` records; the catalog rows are written
//! out in each case's expectation.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agent_client_protocol::schema::v1::{
    AvailableCommand, AvailableCommandsUpdate, ConfigOptionUpdate, ContentBlock, ContentChunk,
    CurrentModeUpdate, ImageContent, MessageId, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, SessionInfoUpdate, SessionUpdate, TextContent, ToolCall, ToolCallId,
    ToolCallStatus as AcpToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    UsageUpdate,
};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use serde_json::json;
use session_event_model::{SessionEvent, SessionItem, SessionItemKind, SkillView};

use super::{ItemProjectionError, ProviderItemProjection};
use crate::{
    CompactionTrigger, EventSinkClosed, HistoryReplayFuture, ProviderBehavior, ProviderError,
    ProviderFacts, ProviderUpdate, SessionEventSink,
};

const HUB: EdgeParty = EdgeParty::Node(ProtocolNode::N5EventHub);

enum Recorded {
    Event(SessionEvent),
    SkillView(SkillView),
}

#[derive(Default)]
struct RecordingSink {
    recorded: Mutex<Vec<Recorded>>,
}

impl SessionEventSink for RecordingSink {
    fn publish_skill_view(&self, _: &str, view: SkillView) -> Result<(), EventSinkClosed> {
        if let Ok(mut recorded) = self.recorded.lock() {
            recorded.push(Recorded::SkillView(view));
        }
        Ok(())
    }

    fn begin_history_replay(&self, _: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if let Ok(mut recorded) = self.recorded.lock() {
            recorded.push(Recorded::Event(event));
        }
        Ok(())
    }
}

/// The provider seam: reports a skill view for command updates and a manual
/// compaction hint for tool `compact-1`, when asked to.
struct ScriptedBehavior {
    skill_view_on_commands: bool,
}

impl ProviderBehavior for ScriptedBehavior {
    fn initialization_authentication(&self) -> crate::ProviderInitializationAuthentication {
        crate::ProviderInitializationAuthentication::NotRequested
    }
    fn can_reuse_setting_confirmation(
        &self,
        _: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        true
    }
    fn model_metadata_source(&self) -> crate::ModelMetadataSource {
        crate::ModelMetadataSource::Unavailable
    }
    fn decode_model_metadata(&self, _: &serde_json::Value) -> crate::ModelReasoningMetadata {
        crate::ModelReasoningMetadata::Unknown(crate::ModelReasoningUnknown::UnsupportedSource)
    }
    fn auto_permission_mode(&self) -> crate::AutoPermissionMode {
        crate::AutoPermissionMode::UNREALIZED
    }
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        Ok(match update {
            ProviderUpdate::Commands { .. } if self.skill_view_on_commands => ProviderFacts {
                skill_view: Some(SkillView::Available {
                    skills: Vec::new(),
                    errors: Vec::new(),
                }),
                compaction: None,
            },
            ProviderUpdate::ToolStatus {
                tool_id, status, ..
            } if tool_id == "compact-1" => ProviderFacts {
                skill_view: None,
                compaction: Some(crate::ProviderCompactionHint {
                    tool_id,
                    status,
                    trigger: CompactionTrigger::Manual,
                }),
            },
            _ => ProviderFacts::default(),
        })
    }
    fn prepare(
        &self,
        _: session_event_model::ProviderSubmission,
    ) -> Result<Vec<session_event_model::PromptContent>, ProviderError> {
        Err(ProviderError::Unsupported)
    }
    fn compact(&self) -> Result<crate::PreparedCompact, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}

struct Harness {
    projection: ProviderItemProjection,
    sink: Arc<RecordingSink>,
    /// item id to the item's variant name, so completions can be labelled.
    base_labels: HashMap<String, String>,
}

impl Harness {
    fn new() -> Self {
        Self::with_behavior(ScriptedBehavior {
            skill_view_on_commands: false,
        })
    }

    fn with_behavior(behavior: ScriptedBehavior) -> Self {
        let sink = Arc::new(RecordingSink::default());
        Self {
            projection: ProviderItemProjection::new(
                "provider-session".into(),
                Arc::clone(&sink) as Arc<dyn SessionEventSink>,
                Arc::new(behavior),
            ),
            sink,
            base_labels: HashMap::new(),
        }
    }

    /// Drives the projection into the case's starting state; its output is not
    /// part of the case.
    fn prime(&mut self, update: &SessionUpdate) {
        self.projection.observe(update, None).unwrap();
        let _ = self.drain();
    }

    /// Takes everything the sink recorded since the last drain (the whole Vec), as
    /// `(kind label, whole value)` pairs: the whole `SessionEvent` as serde JSON, or the
    /// whole skill-view publication as its full `Debug` text (`SkillView` has no serde).
    fn drain(&mut self) -> Vec<(String, serde_json::Value)> {
        let recorded = std::mem::take(&mut *self.sink.recorded.lock().unwrap());
        recorded
            .into_iter()
            .map(|entry| match entry {
                Recorded::SkillView(view) => (
                    format!("SkillViewPublished({})", skill_view_label(&view)),
                    json!({ "debug": format!("{view:?}") }),
                ),
                Recorded::Event(event) => {
                    let label = match &event {
                        SessionEvent::ItemStarted { item } => {
                            self.base_labels
                                .insert(item.item_id.clone(), base_label(item));
                            format!("ItemStarted({})", detailed_label(item))
                        }
                        SessionEvent::ItemUpdated { item } => {
                            self.base_labels
                                .insert(item.item_id.clone(), base_label(item));
                            format!("ItemUpdated({})", detailed_label(item))
                        }
                        SessionEvent::ItemCompleted { item_id } => format!(
                            "ItemCompleted({})",
                            self.base_labels
                                .get(item_id)
                                .map_or("unknown item", String::as_str)
                        ),
                        other => panic!("unexpected event from N6: {other:?}"),
                    };
                    (label, whole_event_value(&event))
                }
            })
            .collect()
    }

    fn snapshot(&self) -> NodeStateSnapshot {
        let projection = &self.projection;
        let mut tools: Vec<_> = projection.tool_items.iter().collect();
        tools.sort_by_key(|(id, _)| id.as_str());
        NodeStateSnapshot::new()
            .field(
                "open_text",
                projection
                    .active_text
                    .as_ref()
                    .map_or("none".to_owned(), |active| text_item_state(&active.item)),
            )
            .field(
                "tool_items",
                if tools.is_empty() {
                    "none".to_owned()
                } else {
                    tools
                        .into_iter()
                        .map(|(id, item)| format!("{id}={}", text_item_state(item)))
                        .collect::<Vec<_>>()
                        .join("; ")
                },
            )
            .field(
                "tool_order",
                if projection.tool_order.is_empty() {
                    "none".to_owned()
                } else {
                    projection.tool_order.join(",")
                },
            )
            .field(
                "plan_item",
                projection
                    .plan_item
                    .as_ref()
                    .map_or("none".to_owned(), |item| {
                        format!("Plan text={:?}", item.text.as_deref().unwrap_or_default())
                    }),
            )
    }

    /// Runs one catalog input through the real node and records both edges and
    /// the declared state.
    fn tap(
        &mut self,
        case_name: &str,
        catalog_row: &str,
        consumed_kind: &str,
        act: impl FnOnce(&mut ProviderItemProjection) -> Result<(), ItemProjectionError>,
    ) -> TapCase {
        let mut case = TapCase::new(ProtocolNode::N6AcpClient, case_name, catalog_row);
        case.state_before(self.snapshot());
        // The update is a typed SDK value handed to the projection: it never crossed a wire here.
        case.consumed(
            RecordLayer::TypedInput,
            MessageFamily::AcpSessionUpdate,
            EdgeParty::AcpAgent,
            consumed_kind,
        );
        act(&mut self.projection).unwrap();
        for (kind, payload) in self.drain() {
            case.produced_with(
                RecordLayer::TypedOutcome,
                MessageFamily::SessionEvent,
                HUB,
                &kind,
                payload,
            );
        }
        case.state_after(self.snapshot());
        case
    }

    fn tap_update(
        &mut self,
        case_name: &str,
        catalog_row: &str,
        consumed_kind: &str,
        update: &SessionUpdate,
    ) -> TapCase {
        self.tap(case_name, catalog_row, consumed_kind, |projection| {
            projection.observe(update, None)
        })
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

fn base_label(item: &SessionItem) -> String {
    match &item.kind {
        SessionItemKind::Unknown { source_kind } => format!("Unknown({source_kind})"),
        other => variant_name(other).to_owned(),
    }
}

/// The kind string of a started or updated item. Metadata items show whether
/// their payload is a JSON string today.
fn detailed_label(item: &SessionItem) -> String {
    let text = item.text.as_deref().unwrap_or_default();
    match &item.kind {
        SessionItemKind::ToolCall { tool_kind, status } => {
            format!("ToolCall({tool_kind},{status:?})")
        }
        SessionItemKind::ModeChange => format!("ModeChange id={text}"),
        SessionItemKind::Usage
        | SessionItemKind::ConfigChange
        | SessionItemKind::SessionInfo
        | SessionItemKind::Notice => {
            let payload = if serde_json::from_str::<serde_json::Value>(text)
                .is_ok_and(|value| value.is_object() || value.is_array())
            {
                "jsonText"
            } else {
                "plainText"
            };
            format!("{} payload={payload}", variant_name(&item.kind))
        }
        other => base_label(&SessionItem {
            item_id: String::new(),
            kind: other.clone(),
            text: None,
        }),
    }
}

fn render_id(item_id: &str) -> String {
    match uuid::Uuid::parse_str(item_id) {
        Ok(id) if id.get_version_num() == 7 => "<uuidv7>".to_owned(),
        _ => item_id.to_owned(),
    }
}

fn text_item_state(item: &SessionItem) -> String {
    format!(
        "{} id={} text={:?}",
        detailed_label(item),
        render_id(&item.item_id),
        item.text.as_deref().unwrap_or("<none>")
    )
}

/// One expected session event: its kind label and whole value, written from literals.
type ExpectedEvent = (String, serde_json::Value);

/// Placeholder for a generated UUIDv7 item id (plan, mode, config, usage and other items
/// without an ACP id are given `Uuid::now_v7()`, so their value differs on every run).
const VOLATILE_ID: &str = "<volatile:itemId>";

/// The whole `SessionEvent` as serde JSON. The only masked field is a generated UUIDv7
/// `itemId`, which differs on every run; ids the agent supplied (`m1`, `t1`) stay literal.
fn whole_event_value(event: &SessionEvent) -> serde_json::Value {
    fn mask(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    let generated = key == "itemId"
                        && field.as_str().is_some_and(|id| {
                            uuid::Uuid::parse_str(id).is_ok_and(|id| id.get_version_num() == 7)
                        });
                    if generated {
                        *field = json!(VOLATILE_ID);
                    } else {
                        mask(field);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(mask),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(event).unwrap();
    mask(&mut value);
    value
}

fn skill_view_label(view: &SkillView) -> &'static str {
    match view {
        SkillView::Unknown => "Unknown",
        SkillView::Available { .. } => "Available",
        SkillView::Unavailable(_) => "Unavailable",
    }
}

/// An item kind without fields, as it serializes: `{"kind": name}`.
fn plain_kind(name: &str) -> serde_json::Value {
    json!({ "kind": name })
}

/// A tool-call item kind as it serializes (`SessionItemKind` renames variants only, so the
/// fields keep their snake_case names).
fn tool_kind_json(tool_kind: &str, status: &str) -> serde_json::Value {
    json!({ "kind": "toolCall", "tool_kind": tool_kind, "status": status })
}

fn item_json(item_id: &str, kind: serde_json::Value, text: Option<&str>) -> serde_json::Value {
    json!({ "itemId": item_id, "kind": kind, "text": text })
}

/// An expected `ItemStarted` (`label` names it) with its whole item.
fn started(
    label: &str,
    item_id: &str,
    kind: serde_json::Value,
    text: Option<&str>,
) -> ExpectedEvent {
    (
        label.to_owned(),
        json!({ "kind": "itemStarted", "item": item_json(item_id, kind, text) }),
    )
}

/// An expected `ItemUpdated` (`label` names it) with its whole item.
fn updated(
    label: &str,
    item_id: &str,
    kind: serde_json::Value,
    text: Option<&str>,
) -> ExpectedEvent {
    (
        label.to_owned(),
        json!({ "kind": "itemUpdated", "item": item_json(item_id, kind, text) }),
    )
}

fn completed(label: &str, item_id: &str) -> ExpectedEvent {
    (
        label.to_owned(),
        json!({ "kind": "itemCompleted", "itemId": item_id }),
    )
}

fn expectation(events: Vec<ExpectedEvent>, state: NodeStateSnapshot) -> CatalogRowExpectation {
    events
        .into_iter()
        .fold(CatalogRowExpectation::new(), |row, (kind, payload)| {
            row.produces_with(MessageFamily::SessionEvent, HUB, &kind, payload)
        })
        .state_after(state)
}

fn state(
    open_text: &str,
    tool_items: &str,
    tool_order: &str,
    plan_item: &str,
) -> NodeStateSnapshot {
    NodeStateSnapshot::new()
        .field("open_text", open_text)
        .field("tool_items", tool_items)
        .field("tool_order", tool_order)
        .field("plan_item", plan_item)
}

fn chunk(text: &str, message_id: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
        .message_id(MessageId::new(message_id))
}

fn agent_message(text: &str, message_id: &str) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(chunk(text, message_id))
}

fn tool_call(id: &str, title: &str, kind: ToolKind, status: AcpToolCallStatus) -> SessionUpdate {
    SessionUpdate::ToolCall(
        ToolCall::new(ToolCallId::new(id), title)
            .kind(kind)
            .status(status),
    )
}

fn tool_update(id: &str, fields: ToolCallUpdateFields) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(ToolCallId::new(id), fields))
}

fn plan(entries: &[(&str, PlanEntryStatus)]) -> SessionUpdate {
    SessionUpdate::Plan(Plan::new(
        entries
            .iter()
            .map(|(content, status)| {
                PlanEntry::new(*content, PlanEntryPriority::Medium, status.clone())
            })
            .collect(),
    ))
}

const NO_STATE: &str = "none";
const OPEN_AGENT_HELLO: &str = "AgentMessage id=m1 text=\"hello\"";

// --- Text chunks -----------------------------------------------------------

#[test]
fn node_tap_user_message_chunk_new_item() {
    let mut harness = Harness::new();
    let case = harness.tap_update(
        "user_message_chunk_new_item",
        "N6 catalog: User / agent message chunk, thought chunk",
        "session/update:user_message_chunk",
        &SessionUpdate::UserMessageChunk(chunk("hi", "u1")),
    );
    let row = expectation(
        vec![started(
            "ItemStarted(UserMessage)",
            "u1",
            plain_kind("userMessage"),
            Some("hi"),
        )],
        state(
            "UserMessage id=u1 text=\"hi\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_message_chunk_new_item() {
    let mut harness = Harness::new();
    let case = harness.tap_update(
        "agent_message_chunk_new_item",
        "N6 catalog: User / agent message chunk, thought chunk",
        "session/update:agent_message_chunk",
        &agent_message("hello", "m1"),
    );
    let row = expectation(
        vec![started(
            "ItemStarted(AgentMessage)",
            "m1",
            plain_kind("agentMessage"),
            Some("hello"),
        )],
        state(OPEN_AGENT_HELLO, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_message_chunk_without_message_id_gets_uuidv7() {
    let mut harness = Harness::new();
    let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
        TextContent::new("hello"),
    )));
    let case = harness.tap_update(
        "agent_message_chunk_without_message_id",
        "N6 catalog: User / agent message chunk, thought chunk (id = message id or a new UUIDv7)",
        "session/update:agent_message_chunk",
        &update,
    );
    let row = expectation(
        vec![started(
            "ItemStarted(AgentMessage)",
            VOLATILE_ID,
            plain_kind("agentMessage"),
            Some("hello"),
        )],
        state(
            "AgentMessage id=<uuidv7> text=\"hello\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_thought_chunk_new_item() {
    let mut harness = Harness::new();
    let case = harness.tap_update(
        "agent_thought_chunk_new_item",
        "N6 catalog: User / agent message chunk, thought chunk",
        "session/update:agent_thought_chunk",
        &SessionUpdate::AgentThoughtChunk(chunk("hmm", "t1")),
    );
    let row = expectation(
        vec![started(
            "ItemStarted(AgentThought)",
            "t1",
            plain_kind("agentThought"),
            Some("hmm"),
        )],
        state(
            "AgentThought id=t1 text=\"hmm\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_message_chunk_continuing_text() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "agent_message_chunk_continuing_text",
        "N6 catalog: continuing text: 1 ItemUpdated with the full text so far",
        "session/update:agent_message_chunk",
        &agent_message(" world", "m1"),
    );
    let row = expectation(
        vec![updated(
            "ItemUpdated(AgentMessage)",
            "m1",
            plain_kind("agentMessage"),
            Some("hello world"),
        )],
        state(
            "AgentMessage id=m1 text=\"hello world\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_thought_chunk_after_message_closes_previous_item() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "agent_thought_chunk_after_message",
        "N6 catalog: new text item: 1 ItemStarted, preceded by 1 ItemCompleted if the previous text item differs",
        "session/update:agent_thought_chunk",
        &SessionUpdate::AgentThoughtChunk(chunk("hmm", "m1")),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(AgentThought)",
                "m1",
                plain_kind("agentThought"),
                Some("hmm"),
            ),
        ],
        state(
            "AgentThought id=m1 text=\"hmm\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_message_chunk_new_message_id_closes_previous_item() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "agent_message_chunk_new_message_id",
        "N6 catalog: new text item: 1 ItemStarted, preceded by 1 ItemCompleted if the previous text item differs",
        "session/update:agent_message_chunk",
        &agent_message("second", "m2"),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(AgentMessage)",
                "m2",
                plain_kind("agentMessage"),
                Some("second"),
            ),
        ],
        state(
            "AgentMessage id=m2 text=\"second\"",
            NO_STATE,
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_agent_message_chunk_non_text_block_closes_open_item() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let image = SessionUpdate::AgentMessageChunk(
        ContentChunk::new(ContentBlock::Image(ImageContent::new("aGk=", "image/png")))
            .message_id(MessageId::new("m1")),
    );
    let case = harness.tap_update(
        "agent_message_chunk_non_text_block",
        "N6 catalog: A non-text block closes the open item",
        "session/update:agent_message_chunk",
        &image,
    );
    let row = expectation(
        vec![completed("ItemCompleted(AgentMessage)", "m1")],
        state(NO_STATE, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}

/// Primes a completed tool call that the provider reports as a manual compaction.
fn harness_with_completed_compaction() -> Harness {
    let mut harness = Harness::new();
    harness.prime(&tool_call(
        "compact-1",
        "Compact",
        ToolKind::Other,
        AcpToolCallStatus::Completed,
    ));
    harness
}

#[path = "provider_item_projection_node_tap_tool_test.rs"]
mod tool_cases;

#[path = "provider_item_projection_node_tap_metadata_test.rs"]
mod metadata_cases;
