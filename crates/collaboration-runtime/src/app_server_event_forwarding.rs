//! Live provider event and interaction projection for the Codex app-server face.
use crate::group_historical_turns;
use crate::render_historical_turn;
use crate::router_session_app_server::thread_alias;
use crate::translate_session_item;
use collaboration_contracts::HubEvent;
use message_board::SessionRef;
use serde_json::{Value, json};
use session_event_model::{SessionEvent, SessionItem, StopReason, TurnLostReason, TurnOutcome};
use std::{collections::HashMap, path::PathBuf};

fn render_turn_event(session: &SessionRef, event: &HubEvent) -> Option<Value> {
    let thread_id = thread_alias(session);
    let (method, turn_id, status) = match &event.event {
        SessionEvent::TurnStarted { turn_id, .. } => ("turn/started", turn_id, "inProgress"),
        SessionEvent::TurnEnded { turn_id, outcome } => {
            let status = match outcome {
                TurnOutcome::Ended {
                    stop_reason: StopReason::Cancelled,
                    ..
                } => "interrupted",
                TurnOutcome::Ended { .. } => "completed",
                TurnOutcome::Lost { .. } => "failed",
            };
            ("turn/completed", turn_id, status)
        }
        _ => return None,
    };
    let error = match &event.event {
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Lost { reason },
            ..
        } => {
            let message = match reason {
                TurnLostReason::ProviderRetired => {
                    "Provider generation retired before the Turn settled"
                }
                TurnLostReason::ProviderTurnFailed => "Provider Turn failed before completion",
                TurnLostReason::EndNotObservable => "Provider Turn end was not observable",
            };
            Some(json!({"message":message,"codexErrorInfo":null}))
        }
        _ => None,
    };
    Some(json!({
        "method":method,
        "params":{
            "threadId":thread_id,
            "turn":{
                "id":turn_id,"items":[],"status":status,"error":error,
                "startedAt":null,"completedAt":null,"durationMs":null
            }
        }
    }))
}

pub(crate) fn historical_turns(session: &SessionRef, snapshot: &[HubEvent]) -> Vec<Value> {
    let mut items = Vec::<SessionItem>::new();
    let mut item_positions = HashMap::<String, usize>::new();
    let mut active_turn: Option<String> = None;
    let mut active_item_start = 0;
    for event in snapshot {
        match &event.event {
            SessionEvent::TurnStarted { turn_id, .. } => {
                active_turn = Some(turn_id.clone());
                active_item_start = items.len();
            }
            SessionEvent::TurnEnded { turn_id, .. } if active_turn.as_deref() == Some(turn_id) => {
                active_turn = None;
            }
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } => {
                if let Some(index) = item_positions.get(&item.item_id).copied() {
                    if let Some(existing) = items.get_mut(index) {
                        *existing = item.clone();
                    }
                } else {
                    item_positions.insert(item.item_id.clone(), items.len());
                    items.push(item.clone());
                }
            }
            _ => {}
        }
    }
    let active_items = active_turn
        .as_ref()
        .map(|_| items.split_off(active_item_start));
    let mut turns = group_historical_turns(session, &items)
        .iter()
        .map(render_historical_turn)
        .filter(|turn| {
            turn.get("items")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        })
        .collect::<Vec<_>>();
    if let (Some(turn_id), Some(active_items)) = (active_turn, active_items) {
        let translated_items = active_items
            .iter()
            .filter_map(|item| translate_session_item(item, &thread_alias(session), &turn_id))
            .map(|translated| translated.thread_item)
            .collect::<Vec<_>>();
        turns.push(json!({
            "id":turn_id,"items":translated_items,"itemsView":"full",
            "status":"inProgress","error":null,
            "startedAt":null,"completedAt":null,"durationMs":null
        }));
    }
    turns
}

pub(crate) struct AppServerEventForwarding {
    pub(crate) user_receipts: crate::native_user_receipts::NativeUserReceipts,
    turn_ids: HashMap<SessionRef, String>,
    items: HashMap<(SessionRef, String), Value>,
    working_directories: HashMap<SessionRef, PathBuf>,
}

impl AppServerEventForwarding {
    pub(crate) fn new() -> Self {
        Self {
            user_receipts: crate::native_user_receipts::NativeUserReceipts::default(),
            turn_ids: HashMap::new(),
            items: HashMap::new(),
            working_directories: HashMap::new(),
        }
    }

    pub(crate) fn register_working_directory(&mut self, session: SessionRef, cwd: PathBuf) {
        self.working_directories.insert(session, cwd);
    }

    pub(crate) fn reset_session(&mut self, session: &SessionRef) -> Vec<Value> {
        self.turn_ids.remove(session);
        self.items
            .retain(|(item_session, _), _| item_session != session);
        let thread_id = thread_alias(session);
        self.user_receipts.complete_turn(&thread_id);
        Vec::new()
    }

    pub(crate) fn project(&mut self, session: &SessionRef, event: &HubEvent) -> Vec<Value> {
        let thread_id = thread_alias(session);
        match &event.event {
            SessionEvent::SettingsChanged { settings } => self
                .working_directories
                .get(session)
                .map(|cwd| {
                    crate::native_session_notifications::settings_notification(
                        &thread_id,
                        session.endpoint.endpoint_id.as_str(),
                        settings,
                        cwd,
                    )
                })
                .into_iter()
                .collect(),
            SessionEvent::StateChanged { state } => {
                crate::native_session_notifications::state_notifications(&thread_id, state)
            }
            SessionEvent::TurnStarted { turn_id, input_id } => {
                self.user_receipts.accept(&thread_id, input_id);
                self.turn_ids.insert(session.clone(), turn_id.clone());
                render_turn_event(session, event).into_iter().collect()
            }
            SessionEvent::TurnEnded { .. } => {
                self.user_receipts.complete_turn(&thread_id);
                self.turn_ids.remove(session);
                self.items
                    .retain(|(item_session, _), _| item_session != session);
                render_turn_event(session, event).into_iter().collect()
            }
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                if matches!(item.kind, session_event_model::SessionItemKind::Usage) =>
            {
                crate::app_server_token_usage::translate_usage_update(
                    item,
                    &thread_id,
                    self.turn_ids.get(session).map(String::as_str),
                )
                .into_iter()
                .collect()
            }
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } => {
                let turn_id = self.turn_ids.get(session).map(String::as_str).unwrap_or("");
                let Some(mut translated) = translate_session_item(item, &thread_id, turn_id) else {
                    self.items.remove(&(session.clone(), item.item_id.clone()));
                    return Vec::new();
                };
                if matches!(item.kind, session_event_model::SessionItemKind::UserMessage) {
                    self.user_receipts.stamp(
                        &thread_id,
                        &item.item_id,
                        matches!(event.event, SessionEvent::ItemStarted { .. }),
                        &mut translated.thread_item,
                        self.items.get(&(session.clone(), item.item_id.clone())),
                    );
                }
                let previous = self.items.insert(
                    (session.clone(), item.item_id.clone()),
                    translated.thread_item.clone(),
                );
                if matches!(&item.kind, session_event_model::SessionItemKind::Plan) {
                    return if previous.as_ref() == Some(&translated.thread_item) {
                        Vec::new()
                    } else {
                        translated.notification.into_iter().collect()
                    };
                }
                let mut frames = if matches!(event.event, SessionEvent::ItemStarted { .. }) {
                    let mut started = vec![json!({"method":"item/started","params":{
                        "threadId":thread_id,"turnId":turn_id,"item":translated.thread_item,
                        "startedAtMs":chrono::Utc::now().timestamp_millis()
                    }})];
                    if let Some(delta) = item.text.as_deref().filter(|text| !text.is_empty()) {
                        match item.kind {
                            session_event_model::SessionItemKind::AgentMessage => {
                                started.push(json!({"method":"item/agentMessage/delta","params":{
                                    "threadId":thread_id,"turnId":turn_id,"itemId":item.item_id,
                                    "delta":delta
                                }}));
                            }
                            session_event_model::SessionItemKind::AgentThought => {
                                started.push(
                                    json!({"method":"item/reasoning/summaryTextDelta","params":{
                                        "threadId":thread_id,"turnId":turn_id,"itemId":item.item_id,
                                        "delta":delta,"summaryIndex":0
                                    }}),
                                );
                            }
                            _ => {}
                        }
                    }
                    started
                } else {
                    match &item.kind {
                        session_event_model::SessionItemKind::AgentMessage => {
                            let old_text = previous
                                .as_ref()
                                .and_then(|value| value.get("text"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let new_text = item.text.as_deref().unwrap_or("");
                            new_text
                                .strip_prefix(old_text)
                                .filter(|delta| !delta.is_empty())
                                .map(|delta| {
                                    vec![json!({"method":"item/agentMessage/delta","params":{
                                        "threadId":thread_id,"turnId":turn_id,"itemId":item.item_id,
                                        "delta":delta
                                    }})]
                                })
                                .unwrap_or_default()
                        }
                        session_event_model::SessionItemKind::AgentThought => {
                            let old_text = previous
                                .as_ref()
                                .and_then(|value| value.get("summary"))
                                .and_then(Value::as_array)
                                .and_then(|parts| parts.first())
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let new_text = item.text.as_deref().unwrap_or("");
                            new_text
                                .strip_prefix(old_text)
                                .filter(|delta| !delta.is_empty())
                                .map(|delta| {
                                    vec![json!({"method":"item/reasoning/summaryTextDelta","params":{
                                        "threadId":thread_id,"turnId":turn_id,"itemId":item.item_id,
                                        "delta":delta,"summaryIndex":0
                                    }})]
                                })
                                .unwrap_or_default()
                        }
                        session_event_model::SessionItemKind::ToolCall { tool_kind, .. }
                            if tool_kind == "contextCompaction" =>
                        {
                            Vec::new()
                        }
                        session_event_model::SessionItemKind::ToolCall { .. } => item
                            .text
                            .as_deref()
                            .filter(|message| !message.is_empty())
                            .map(|message| {
                                vec![json!({"method":"item/mcpToolCall/progress","params":{
                                    "threadId":thread_id,"turnId":turn_id,"itemId":item.item_id,
                                    "message":message
                                }})]
                            })
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    }
                };
                if let Some(notification) = translated.notification {
                    frames.push(notification);
                }
                frames
            }
            SessionEvent::ItemCompleted { item_id } => {
                let Some(item) = self.items.remove(&(session.clone(), item_id.clone())) else {
                    return Vec::new();
                };
                if item.get("type").and_then(Value::as_str) == Some("plan") {
                    return Vec::new();
                }
                let Some(turn_id) = self.turn_ids.get(session) else {
                    return Vec::new();
                };
                vec![json!({"method":"item/completed","params":{
                    "threadId":thread_id,"turnId":turn_id,"item":item,
                    "completedAtMs":chrono::Utc::now().timestamp_millis()
                }})]
            }
            SessionEvent::InputAccepted { input_id, .. } => {
                self.user_receipts.accept(&thread_id, input_id);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "app_server_event_forwarding_test.rs"]
mod notification_wire_tests;

#[cfg(test)]
#[path = "app_server_metadata_projection_test.rs"]
mod metadata_projection_tests;

#[cfg(test)]
#[path = "app_server_event_forwarding_node_tap_test.rs"]
mod node_tap_tests;

#[cfg(test)]
#[path = "app_server_event_forwarding_node_tap_item_test.rs"]
mod node_tap_item_tests;
