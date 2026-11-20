//! Keep Cursor plan and todo updates on one stable Session Item.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use provider_session_machines::session_turn_machine::TurnId;
use serde::Deserialize;
use session_event_model::{SessionEvent, SessionItem, SessionItemKind};
use tokio_util::sync::CancellationToken;

use crate::{
    SessionEventSink, acp_transport::TransportRun, owner_callback_routing::OwnerCallbackRouting,
};

#[derive(Default)]
pub(crate) struct CursorPlanItems(
    Mutex<BTreeMap<(TransportRun, TurnId, String), CursorPlanRecord>>,
);

#[derive(Default)]
struct CursorPlanRecord {
    name: Option<String>,
    overview: Option<String>,
    markdown: Option<String>,
    todos: Vec<CursorTodo>,
}

pub(crate) struct CursorPlanDefinition {
    pub(crate) tool_call_id: String,
    pub(crate) name: Option<String>,
    pub(crate) overview: Option<String>,
    pub(crate) markdown: String,
    pub(crate) todos: Vec<CursorTodo>,
}

impl CursorPlanItems {
    pub(crate) fn forget_run(&self, run: TransportRun) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(saved_run, _, _), _| *saved_run != run);
    }

    pub(crate) fn forget_turn(&self, run: TransportRun, turn: TurnId) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(saved_run, saved_turn, _), _| (*saved_run, *saved_turn) != (run, turn));
    }

    pub(crate) fn set_plan(
        &self,
        run: TransportRun,
        turn: TurnId,
        plan: CursorPlanDefinition,
    ) -> (bool, SessionItem) {
        let mut plans = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (run, turn, plan.tool_call_id);
        let existed = plans.contains_key(&key);
        let record = plans.entry(key.clone()).or_default();
        record.name = plan.name;
        record.overview = plan.overview;
        record.markdown = Some(plan.markdown);
        record.todos = plan.todos;
        (existed, record.item(&key.2))
    }

    fn update(
        &self,
        run: TransportRun,
        turn: TurnId,
        request: CursorTodoUpdate,
    ) -> (bool, SessionItem) {
        let mut plans = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (run, turn, request.tool_call_id);
        let existed = plans.contains_key(&key);
        let record = plans.entry(key.clone()).or_default();
        if !request.merge {
            record.todos.clear();
        }
        for update in request.todos {
            if let Some(existing) = record.todos.iter_mut().find(|todo| todo.id == update.id) {
                *existing = update;
            } else {
                record.todos.push(update);
            }
        }
        (existed, record.item(&key.2))
    }
}

impl CursorPlanRecord {
    fn item(&self, tool_call_id: &str) -> SessionItem {
        let mut sections = Vec::new();
        if let Some(name) = &self.name {
            sections.push(format!("# {name}"));
        }
        if let Some(overview) = &self.overview {
            sections.push(overview.clone());
        }
        if let Some(markdown) = &self.markdown {
            sections.push(markdown.clone());
        }
        if !self.todos.is_empty() {
            sections.push(
                self.todos
                    .iter()
                    .map(|todo| format!("- {}: {}", todo.status.label(), todo.content))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        SessionItem {
            item_id: format!("plan:{tool_call_id}"),
            kind: SessionItemKind::Plan,
            text: Some(sections.join("\n\n")),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorTodoUpdate {
    tool_call_id: String,
    todos: Vec<CursorTodo>,
    #[serde(default)]
    merge: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct CursorTodo {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) status: CursorTodoStatus,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CursorTodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl CursorTodoStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in progress",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

pub(crate) struct ProviderCursorTodoHandler {
    tool_registry: Arc<OwnerCallbackRouting>,
    todo_state: Arc<CursorPlanItems>,
    event_sink: Arc<dyn SessionEventSink>,
    shutdown: CancellationToken,
    sink_closed: CancellationToken,
}

impl ProviderCursorTodoHandler {
    pub(crate) fn new(
        tool_registry: Arc<OwnerCallbackRouting>,
        todo_state: Arc<CursorPlanItems>,
        event_sink: Arc<dyn SessionEventSink>,
        shutdown: CancellationToken,
        sink_closed: CancellationToken,
    ) -> Self {
        Self {
            tool_registry,
            todo_state,
            event_sink,
            shutdown,
            sink_closed,
        }
    }

    fn apply_update(&self, params: serde_json::Value) {
        let Ok(update) = serde_json::from_value::<CursorTodoUpdate>(params) else {
            tracing::warn!("malformed Cursor todo update");
            return;
        };
        let Some(crate::owner_callback_routing::ToolCallOwner {
            session_id,
            run,
            turn,
        }) = self.tool_registry.resolve_session(&update.tool_call_id)
        else {
            tracing::warn!("unresolved Cursor todo update");
            return;
        };
        let (existed, item) = self.todo_state.update(run, turn, update);
        let event = if existed {
            SessionEvent::ItemUpdated { item }
        } else {
            SessionEvent::ItemStarted { item }
        };
        // In the run that announced the tool call (D1 R14; review F4).
        if self
            .event_sink
            .publish_for_turn(&session_id, run.generation, turn, event)
            .is_err()
        {
            self.sink_closed.cancel();
            self.shutdown.cancel();
        }
    }
}

impl HandleDispatchFrom<Agent> for ProviderCursorTodoHandler {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        match message {
            Dispatch::Request(request, responder) if request.method() == "cursor/update_todos" => {
                self.apply_update(request.params().clone());
                responder.respond(serde_json::json!({}))?;
                Ok(Handled::Yes)
            }
            Dispatch::Notification(notification)
                if notification.method() == "cursor/update_todos" =>
            {
                self.apply_update(notification.params().clone());
                Ok(Handled::Yes)
            }
            message => Ok(Handled::No {
                message,
                retry: false,
            }),
        }
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "ProviderCursorTodoHandler"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CursorPlanItems, CursorTodo, CursorTodoStatus, CursorTodoUpdate, TransportRun, TurnId,
    };
    use provider_session_machines::{SessionGeneration, session_owner::OwnerIncarnation};

    #[test]
    fn merge_updates_existing_todos_and_preserves_the_plan_item_id() {
        let state = CursorPlanItems::default();
        let run = TransportRun {
            incarnation: OwnerIncarnation(uuid::Uuid::now_v7()),
            generation: SessionGeneration::from_number(1),
        };
        let turn = TurnId(uuid::Uuid::now_v7());
        let first = state.update(
            run,
            turn,
            CursorTodoUpdate {
                tool_call_id: "tool".to_owned(),
                todos: vec![CursorTodo {
                    id: "a".to_owned(),
                    content: "First".to_owned(),
                    status: CursorTodoStatus::Pending,
                }],
                merge: false,
            },
        );
        let second = state.update(
            run,
            turn,
            CursorTodoUpdate {
                tool_call_id: "tool".to_owned(),
                todos: vec![
                    CursorTodo {
                        id: "a".to_owned(),
                        content: "First revised".to_owned(),
                        status: CursorTodoStatus::Completed,
                    },
                    CursorTodo {
                        id: "b".to_owned(),
                        content: "Second".to_owned(),
                        status: CursorTodoStatus::InProgress,
                    },
                ],
                merge: true,
            },
        );
        assert!(!first.0);
        assert!(second.0);
        assert_eq!(first.1.item_id, second.1.item_id);
        assert_eq!(
            second.1.text.as_deref(),
            Some("- completed: First revised\n- in progress: Second")
        );
        let successor = TransportRun {
            incarnation: OwnerIncarnation(uuid::Uuid::now_v7()),
            ..run
        };
        let empty_update = || CursorTodoUpdate {
            tool_call_id: "tool".to_owned(),
            todos: Vec::new(),
            merge: true,
        };
        assert!(!state.update(successor, turn, empty_update()).0);
        state.forget_run(run);
        assert!(state.update(successor, turn, empty_update()).0);
        state.forget_turn(successor, turn);
        let after_end = state.update(successor, turn, empty_update());
        assert!(!after_end.0);
    }
}
