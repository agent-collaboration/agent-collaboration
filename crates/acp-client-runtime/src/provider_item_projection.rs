//! Project streamed ACP updates into stable, ordered Session Items.

use std::{collections::HashMap, sync::Arc};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionUpdate, ToolCall, ToolCallStatus as AcpToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use session_event_model::{SessionEvent, SessionItem, SessionItemKind, ToolCallStatus};

use crate::{EventSinkClosed, SessionEventSink, agent_session_client::MAX_PROMPT_OUTPUT_BYTES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ItemProjectionError {
    OutputLimit,
    SinkClosed,
}

impl From<EventSinkClosed> for ItemProjectionError {
    fn from(_: EventSinkClosed) -> Self {
        Self::SinkClosed
    }
}

struct ActiveTextItem {
    item: SessionItem,
    message_id: Option<String>,
}

pub(crate) struct ProviderItemProjection {
    behavior: Arc<dyn crate::ProviderBehavior>,
    skill_view: session_event_model::SkillView,
    compaction_hints: HashMap<String, crate::ProviderCompactionHint>,
    invalid_compaction: bool,
    session_id: String,
    event_sink: Arc<dyn SessionEventSink>,
    active_text: Option<ActiveTextItem>,
    tool_items: HashMap<String, SessionItem>,
    tool_order: Vec<String>,
    plan_item: Option<SessionItem>,
}

impl ProviderItemProjection {
    pub(crate) fn admit(&mut self) -> Result<(), ItemProjectionError> {
        self.apply_provider_update(crate::ProviderUpdate::SessionAdmitted, Vec::new())
    }

    pub(crate) fn validates(&self, submission: &session_event_model::ProviderSubmission) -> bool {
        submission
            .selected
            .iter()
            .all(|selection| self.skill_view.contains_selection(selection))
    }

    pub(crate) fn compact_confirmed(&self) -> bool {
        !self.invalid_compaction
            && self.compaction_hints.iter().any(|(id, hint)| {
                matches!(
                    hint.trigger,
                    crate::CompactionTrigger::Manual | crate::CompactionTrigger::Unlabelled
                ) && self.tool_items.get(id).is_some_and(|item| {
                    matches!(
                        item.kind,
                        SessionItemKind::ToolCall {
                            status: ToolCallStatus::Completed,
                            ..
                        }
                    )
                })
            })
    }

    pub(crate) fn promote_compaction(&mut self) -> Result<(), ItemProjectionError> {
        let completed = self
            .compaction_hints
            .iter()
            .filter_map(|(id, hint)| {
                (matches!(
                    hint.trigger,
                    crate::CompactionTrigger::Manual | crate::CompactionTrigger::Unlabelled
                ) && self.tool_items.get(id).is_some_and(|item| {
                    matches!(
                        item.kind,
                        SessionItemKind::ToolCall {
                            status: ToolCallStatus::Completed,
                            ..
                        }
                    )
                }))
                .then_some(id.clone())
            })
            .collect::<Vec<_>>();
        for id in completed {
            if let Some(item) = self.tool_items.get_mut(&id) {
                item.kind = SessionItemKind::ToolCall {
                    tool_kind: "contextCompaction".into(),
                    status: ToolCallStatus::Completed,
                };
                let item = item.clone();
                self.emit(SessionEvent::ItemUpdated { item })?;
            }
        }
        Ok(())
    }

    fn apply_provider_update(
        &mut self,
        update: crate::ProviderUpdate,
        row_errors: Vec<session_event_model::SkillRowError>,
    ) -> Result<(), ItemProjectionError> {
        let tool_update = matches!(update, crate::ProviderUpdate::ToolStatus { .. });
        match self.behavior.observe(update) {
            Ok(facts) => {
                if let Some(mut view) = facts.skill_view {
                    if let session_event_model::SkillView::Available { errors, .. } = &mut view {
                        errors.extend(row_errors);
                    }
                    if self.skill_view != view {
                        self.event_sink
                            .publish_skill_view(&self.session_id, view.clone())?;
                        self.skill_view = view;
                    }
                }
                if let Some(hint) = facts.compaction {
                    self.compaction_hints.insert(hint.tool_id.clone(), hint);
                }
            }
            Err(_) if tool_update => self.invalid_compaction = true,
            Err(_) => {
                let view = session_event_model::SkillView::Unavailable(
                    session_event_model::SkillSourceError::MalformedMetadata,
                );
                self.event_sink
                    .publish_skill_view(&self.session_id, view.clone())?;
                self.skill_view = view;
            }
        }
        Ok(())
    }

    fn observe_provider_update(
        &mut self,
        update: &SessionUpdate,
        raw: Option<&serde_json::Value>,
    ) -> Result<(), ItemProjectionError> {
        let mut row_errors = Vec::new();
        let neutral = match update {
            SessionUpdate::AvailableCommandsUpdate(commands) => crate::ProviderUpdate::Commands {
                commands: if let Some(raw) = raw {
                    let Some(rows) = raw
                        .get("availableCommands")
                        .and_then(serde_json::Value::as_array)
                    else {
                        let view = session_event_model::SkillView::Unavailable(
                            session_event_model::SkillSourceError::MalformedMetadata,
                        );
                        self.event_sink
                            .publish_skill_view(&self.session_id, view.clone())?;
                        self.skill_view = view;
                        return Ok(());
                    };
                    rows.iter().filter_map(|raw_row| {
                        match serde_json::from_value::<agent_client_protocol::schema::v1::AvailableCommand>(raw_row.clone()) {
                            Ok(row) => Some(crate::ProviderCommand {
                                invocation_name: row.name,
                                description: row.description,
                                input_hint: match row.input { Some(agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(input)) => Some(input.hint), _ => None },
                                metadata: raw_row.get("_meta").cloned(),
                            }),
                            Err(_) => {
                                row_errors.push(session_event_model::SkillRowError { advertised_name: raw_row.get("name").and_then(serde_json::Value::as_str).unwrap_or("<invalid>").into(), reason: session_event_model::SkillRowErrorReason::MalformedMetadata });
                                None
                            }
                        }
                    }).collect()
                } else {
                    commands.available_commands.iter().map(|row| crate::ProviderCommand {
                    invocation_name: row.name.clone(), description: row.description.clone(),
                    input_hint: match &row.input {
                        Some(agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(input)) => Some(input.hint.clone()),
                        _ => None,
                    },
                    metadata: row.meta.clone().map(serde_json::Value::Object),
                }).collect()
                },
                metadata: raw
                    .and_then(|update| update.get("_meta"))
                    .cloned()
                    .or_else(|| commands.meta.clone().map(serde_json::Value::Object)),
            },
            SessionUpdate::ToolCall(tool) => crate::ProviderUpdate::ToolStatus {
                tool_id: tool.tool_call_id.0.to_string(),
                status: tool_status(tool.status),
                metadata: tool.meta.clone().map(serde_json::Value::Object),
            },
            SessionUpdate::ToolCallUpdate(tool) => {
                let current_status = self
                    .tool_items
                    .get(tool.tool_call_id.0.as_ref())
                    .and_then(|item| match item.kind {
                        SessionItemKind::ToolCall { status, .. } => Some(status),
                        _ => None,
                    })
                    .unwrap_or(ToolCallStatus::Pending);
                crate::ProviderUpdate::ToolStatus {
                    tool_id: tool.tool_call_id.0.to_string(),
                    status: tool.fields.status.map_or(current_status, tool_status),
                    metadata: tool.meta.clone().map(serde_json::Value::Object),
                }
            }
            _ => return Ok(()),
        };
        self.apply_provider_update(neutral, row_errors)
    }

    pub(crate) fn new(
        session_id: String,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
    ) -> Self {
        Self {
            behavior,
            skill_view: session_event_model::SkillView::Unknown,
            compaction_hints: HashMap::new(),
            invalid_compaction: false,
            session_id,
            event_sink,
            active_text: None,
            tool_items: HashMap::new(),
            tool_order: Vec::new(),
            plan_item: None,
        }
    }

    fn emit(&self, event: SessionEvent) -> Result<(), ItemProjectionError> {
        self.event_sink
            .publish(&self.session_id, event)
            .map_err(Into::into)
    }

    pub(crate) fn observe(
        &mut self,
        update: &SessionUpdate,
        raw: Option<&serde_json::Value>,
    ) -> Result<(), ItemProjectionError> {
        self.observe_provider_update(update, raw)?;
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                self.observe_text(chunk, SessionItemKind::UserMessage)
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.observe_text(chunk, SessionItemKind::AgentMessage)
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.observe_text(chunk, SessionItemKind::AgentThought)
            }
            SessionUpdate::ToolCall(tool_call) => {
                self.finish_text()?;
                self.observe_tool_call(tool_call)
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.finish_text()?;
                self.observe_tool_update(update)
            }
            SessionUpdate::Plan(plan) => {
                self.finish_text()?;
                self.observe_plan(plan)
            }
            SessionUpdate::CurrentModeUpdate(mode) => self.emit_once(
                SessionItemKind::ModeChange,
                mode.current_mode_id.0.to_string(),
            ),
            SessionUpdate::ConfigOptionUpdate(config) => self.emit_once(
                SessionItemKind::ConfigChange,
                serde_json::to_string(&config.config_options).unwrap_or_default(),
            ),
            SessionUpdate::SessionInfoUpdate(info) => self.emit_once(
                SessionItemKind::SessionInfo,
                serde_json::to_string(info).unwrap_or_default(),
            ),
            SessionUpdate::UsageUpdate(usage) => self.emit_once(
                SessionItemKind::Usage,
                serde_json::to_string(usage).unwrap_or_default(),
            ),
            SessionUpdate::AvailableCommandsUpdate(commands) => self.emit_once(
                SessionItemKind::Notice,
                serde_json::to_string(commands).unwrap_or_default(),
            ),
            _ => self.finish_text(),
        }
    }

    fn observe_text(
        &mut self,
        chunk: &ContentChunk,
        kind: SessionItemKind,
    ) -> Result<(), ItemProjectionError> {
        let ContentBlock::Text(text) = &chunk.content else {
            self.finish_text()?;
            return Ok(());
        };
        let message_id = chunk.message_id.as_ref().map(|id| id.0.to_string());
        if self
            .active_text
            .as_ref()
            .is_some_and(|active| active.item.kind != kind || active.message_id != message_id)
        {
            self.finish_text()?;
        }
        if let Some(active) = &mut self.active_text {
            let prior = active.item.text.as_deref().unwrap_or_default();
            if prior.len().saturating_add(text.text.len()) > MAX_PROMPT_OUTPUT_BYTES {
                return Err(ItemProjectionError::OutputLimit);
            }
            active
                .item
                .text
                .get_or_insert_with(String::new)
                .push_str(&text.text);
            let updated_item = active.item.clone();
            self.emit(SessionEvent::ItemUpdated { item: updated_item })
        } else {
            if text.text.len() > MAX_PROMPT_OUTPUT_BYTES {
                return Err(ItemProjectionError::OutputLimit);
            }
            let item = SessionItem {
                item_id: message_id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                kind,
                text: Some(text.text.clone()),
            };
            self.emit(SessionEvent::ItemStarted { item: item.clone() })?;
            self.active_text = Some(ActiveTextItem { item, message_id });
            Ok(())
        }
    }

    pub(crate) fn discard_turn_local_cursors(&mut self) {
        self.compaction_hints.clear();
        self.invalid_compaction = false;
        self.active_text = None;
        self.tool_order.clear();
        self.plan_item = None;
    }

    pub(crate) fn finish_text(&mut self) -> Result<(), ItemProjectionError> {
        if let Some(active) = self.active_text.take() {
            self.emit(SessionEvent::ItemCompleted {
                item_id: active.item.item_id,
            })?;
        }
        Ok(())
    }

    fn observe_tool_call(&mut self, tool_call: &ToolCall) -> Result<(), ItemProjectionError> {
        let item_id = tool_call.tool_call_id.0.to_string();
        let item = SessionItem {
            item_id: item_id.clone(),
            kind: SessionItemKind::ToolCall {
                tool_kind: if self.tool_items.get(&item_id).is_some_and(|item| matches!(&item.kind, SessionItemKind::ToolCall { tool_kind, .. } if tool_kind == "contextCompaction")) {
                    "contextCompaction".into()
                } else {
                    tool_kind(tool_call.kind).to_owned()
                },
                status: tool_status(tool_call.status),
            },
            text: Some(tool_call.title.clone()),
        };
        let existed = self
            .tool_items
            .insert(item_id.clone(), item.clone())
            .is_some();
        if !existed {
            self.tool_order.push(item_id);
        }
        self.emit(if existed {
            SessionEvent::ItemUpdated { item }
        } else {
            SessionEvent::ItemStarted { item }
        })
    }

    fn observe_tool_update(&mut self, update: &ToolCallUpdate) -> Result<(), ItemProjectionError> {
        let item_id = update.tool_call_id.0.to_string();
        let existed = self.tool_items.contains_key(&item_id);
        let item = self
            .tool_items
            .entry(item_id.clone())
            .or_insert_with(|| SessionItem {
                item_id: item_id.clone(),
                kind: SessionItemKind::ToolCall {
                    tool_kind: "other".to_owned(),
                    status: ToolCallStatus::Pending,
                },
                text: None,
            });
        if let SessionItemKind::ToolCall {
            tool_kind: current_kind,
            status,
        } = &mut item.kind
        {
            if *current_kind != "contextCompaction"
                && let Some(kind) = update.fields.kind
            {
                *current_kind = tool_kind(kind).to_owned();
            }
            if let Some(updated) = update.fields.status {
                *status = tool_status(updated);
            }
        }
        if let Some(title) = &update.fields.title {
            item.text = Some(title.clone());
        }
        let item = item.clone();
        if !existed {
            self.tool_order.push(item_id);
        }
        self.emit(if existed {
            SessionEvent::ItemUpdated { item }
        } else {
            SessionEvent::ItemStarted { item }
        })
    }

    fn observe_plan(
        &mut self,
        plan: &agent_client_protocol::schema::v1::Plan,
    ) -> Result<(), ItemProjectionError> {
        let text = plan
            .entries
            .iter()
            .map(|entry| format!("- {:?}: {}", entry.status, entry.content))
            .collect::<Vec<_>>()
            .join("\n");
        let existed = self.plan_item.is_some();
        let item = SessionItem {
            item_id: self.plan_item.as_ref().map_or_else(
                || uuid::Uuid::now_v7().to_string(),
                |item| item.item_id.clone(),
            ),
            kind: SessionItemKind::Plan,
            text: Some(text),
        };
        self.plan_item = Some(item.clone());
        self.emit(if existed {
            SessionEvent::ItemUpdated { item }
        } else {
            SessionEvent::ItemStarted { item }
        })
    }

    pub(crate) fn finish(&mut self) -> Result<(), ItemProjectionError> {
        self.finish_text()?;
        for item_id in std::mem::take(&mut self.tool_order) {
            self.emit(SessionEvent::ItemCompleted { item_id })?;
        }
        if let Some(plan) = self.plan_item.take() {
            self.emit(SessionEvent::ItemCompleted {
                item_id: plan.item_id,
            })?;
        }
        Ok(())
    }

    pub(crate) fn observe_unknown(
        &mut self,
        source_kind: &str,
        content: Option<&str>,
    ) -> Result<(), ItemProjectionError> {
        self.finish_text()?;
        let text = content.unwrap_or("Unrecognized agent update");
        if text.len() > MAX_PROMPT_OUTPUT_BYTES {
            return Err(ItemProjectionError::OutputLimit);
        }
        let item = SessionItem {
            item_id: uuid::Uuid::now_v7().to_string(),
            kind: SessionItemKind::Unknown {
                source_kind: source_kind.to_owned(),
            },
            text: Some(text.to_owned()),
        };
        self.emit(SessionEvent::ItemStarted { item: item.clone() })?;
        self.emit(SessionEvent::ItemCompleted {
            item_id: item.item_id,
        })
    }

    fn emit_once(
        &mut self,
        kind: SessionItemKind,
        text: String,
    ) -> Result<(), ItemProjectionError> {
        self.finish_text()?;
        if text.len() > MAX_PROMPT_OUTPUT_BYTES {
            return Err(ItemProjectionError::OutputLimit);
        }
        let item = SessionItem {
            item_id: uuid::Uuid::now_v7().to_string(),
            kind,
            text: Some(text),
        };
        self.emit(SessionEvent::ItemStarted { item: item.clone() })?;
        self.emit(SessionEvent::ItemCompleted {
            item_id: item.item_id,
        })
    }
}

const fn tool_kind(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "execute",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        _ => "other",
    }
}

const fn tool_status(status: AcpToolCallStatus) -> ToolCallStatus {
    match status {
        AcpToolCallStatus::Pending => ToolCallStatus::Pending,
        AcpToolCallStatus::InProgress => ToolCallStatus::InProgress,
        AcpToolCallStatus::Completed => ToolCallStatus::Completed,
        AcpToolCallStatus::Failed => ToolCallStatus::Failed,
        _ => ToolCallStatus::Pending,
    }
}
