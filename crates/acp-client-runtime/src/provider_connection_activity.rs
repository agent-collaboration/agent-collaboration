//! Connection-local tool ownership and active turn routing.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_lifecycle_machine::SessionLifecycle;
use provider_session_machines::session_turn_machine::TurnId;
use provider_session_machines::tool_ownership::{
    ToolObservation, ToolOwnership, ToolOwnershipEffect, ToolOwnershipEvent, ToolSessionPhase,
    reduce_tool_ownership,
};

use crate::session_cell_table::SessionCellTable;

pub(crate) struct ProviderConnectionActivity {
    session_cells: Arc<SessionCellTable>,
    state: Mutex<RegistryState>,
}

struct RunningTurn {
    turn: TurnId,
    generation: SessionGeneration,
}

/// The session and generation that may receive a tool-scoped callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ToolCallOwner {
    pub(crate) session_id: String,
    pub(crate) generation: SessionGeneration,
}

#[derive(Default)]
struct RegistryState {
    running_sessions: HashMap<String, RunningTurn>,
    /// The outer key is the tool ID because Cursor callbacks omit sessionId.
    /// Empty maps are never stored, so only a missing key permits fallback.
    tool_owners: HashMap<String, HashMap<String, ToolOwnership>>,
    /// A retirement visits the session's active claims, independent of how
    /// many historical IDs the connection has seen.
    active_tool_ids: HashMap<String, HashSet<String>>,
}

impl RegistryState {
    fn phase(&self, session_id: &str, lifecycle: &SessionLifecycle) -> Option<ToolSessionPhase> {
        match *lifecycle {
            SessionLifecycle::Loading { generation, .. } => {
                Some(ToolSessionPhase::Loading { generation })
            }
            SessionLifecycle::Live { generation } => match self.running_sessions.get(session_id) {
                Some(running) if running.generation == generation => {
                    Some(ToolSessionPhase::LiveRunning {
                        generation,
                        turn: running.turn,
                    })
                }
                _ => Some(ToolSessionPhase::LiveIdle { generation }),
            },
            SessionLifecycle::NotLoaded { .. } | SessionLifecycle::Closing { .. } => None,
        }
    }

    fn update_owners(&mut self, session_id: &str, event: ToolOwnershipEvent) {
        let Some(active_ids) = self.active_tool_ids.get_mut(session_id) else {
            return;
        };
        active_ids.retain(|tool_id| {
            let Some(owner) = self
                .tool_owners
                .get_mut(tool_id)
                .and_then(|owners| owners.get_mut(session_id))
            else {
                return false;
            };
            let (Some(updated), _) = reduce_tool_ownership(Some(*owner), event) else {
                return false;
            };
            *owner = updated;
            owner.owned_generation().is_some()
        });
        if active_ids.is_empty() {
            self.active_tool_ids.remove(session_id);
        }
    }

    fn callback_candidate(&self, tool_id: &str) -> Option<&str> {
        if let Some(owners) = self.tool_owners.get(tool_id) {
            let mut candidates = owners.iter().filter_map(|(session_id, owner)| {
                owner.owned_generation().map(|_| session_id.as_str())
            });
            let candidate = candidates.next()?;
            return candidates.next().is_none().then_some(candidate);
        }
        let mut sessions = self.running_sessions.keys();
        let candidate = sessions.next()?;
        sessions.next().is_none().then_some(candidate.as_str())
    }
}

impl ProviderConnectionActivity {
    pub(crate) fn new(session_cells: Arc<SessionCellTable>) -> Self {
        Self {
            session_cells,
            state: Mutex::new(RegistryState::default()),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, RegistryState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn turn_started(
        &self,
        session_id: &str,
        turn: TurnId,
        generation: SessionGeneration,
    ) {
        self.lock_state()
            .running_sessions
            .insert(session_id.to_owned(), RunningTurn { turn, generation });
    }

    fn observe_tool(&self, session_id: &str, tool_id: &str, observation: ToolObservation) -> bool {
        self.session_cells.read(session_id, |cell| {
            // Use the cell, then the registry. A retired run cannot record
            // ownership after the cell admits a newer run.
            let mut state = self.lock_state();
            let Some(phase) = state.phase(session_id, &cell.lifecycle) else {
                return false;
            };
            let previous = state
                .tool_owners
                .get(tool_id)
                .and_then(|owners| owners.get(session_id))
                .copied();
            let (owner, effect) =
                reduce_tool_ownership(previous, ToolOwnershipEvent::Observe { phase, observation });
            if owner != previous
                && let Some(owner) = owner
            {
                state
                    .tool_owners
                    .entry(tool_id.to_owned())
                    .or_default()
                    .insert(session_id.to_owned(), owner);
                state
                    .active_tool_ids
                    .entry(session_id.to_owned())
                    .or_default()
                    .insert(tool_id.to_owned());
            }
            matches!(effect, ToolOwnershipEffect::ForwardFrame { .. })
        })
    }

    pub(crate) fn resolve_session(&self, tool_id: &str) -> Option<ToolCallOwner> {
        let candidate = self.lock_state().callback_candidate(tool_id)?.to_owned();
        self.session_cells.read(&candidate, |cell| {
            let state = self.lock_state();
            // Revalidate after taking the cell lock so a changed candidate
            // doesn't inherit the earlier result or invert the lock order.
            if state.callback_candidate(tool_id) != Some(candidate.as_str()) {
                return None;
            }
            let phase = state.phase(&candidate, &cell.lifecycle)?;
            let owner = state
                .tool_owners
                .get(tool_id)
                .and_then(|owners| owners.get(&candidate))
                .copied();
            let (_, effect) = reduce_tool_ownership(owner, ToolOwnershipEvent::Callback { phase });
            match effect {
                ToolOwnershipEffect::ResolveOwner { generation }
                | ToolOwnershipEffect::UseSoleRunningTurn { generation } => Some(ToolCallOwner {
                    session_id: candidate.clone(),
                    generation,
                }),
                _ => None,
            }
        })
    }

    pub(crate) fn retire_run(&self, session_id: &str, generation: SessionGeneration) {
        let mut state = self.lock_state();
        if state
            .running_sessions
            .get(session_id)
            .is_some_and(|run| run.generation == generation)
        {
            state.running_sessions.remove(session_id);
        }
        state.update_owners(session_id, ToolOwnershipEvent::RunRetired { generation });
    }

    pub(crate) fn claim_turn_end(&self, session_id: &str, turn: TurnId) -> bool {
        let mut state = self.lock_state();
        let Some(running) = state
            .running_sessions
            .get(session_id)
            .filter(|running| running.turn == turn)
        else {
            return false;
        };
        let generation = running.generation;
        state.running_sessions.remove(session_id);
        state.update_owners(
            session_id,
            ToolOwnershipEvent::TurnEnded { generation, turn },
        );
        true
    }

    pub(crate) fn drain_running_turns(&self) -> Vec<(String, String)> {
        let mut state = self.lock_state();
        state.tool_owners.clear();
        state.active_tool_ids.clear();
        state
            .running_sessions
            .drain()
            .map(|(session_id, run)| (session_id, run.turn.0.to_string()))
            .collect()
    }
}

/// Observe tool ownership before the SDK hands a session/update to its
/// Session actor. Cursor may send a connection request immediately afterward.
pub(crate) struct ToolCallOwnershipHandler(Arc<ProviderConnectionActivity>);

impl ToolCallOwnershipHandler {
    pub(crate) fn new(registry: Arc<ProviderConnectionActivity>) -> Self {
        Self(registry)
    }
}

impl HandleDispatchFrom<Agent> for ToolCallOwnershipHandler {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        if let Dispatch::Notification(notification) = &message
            && notification.method() == "session/update"
        {
            let params = notification.params();
            let update = params.get("update");
            let kind = update
                .and_then(|update| update.get("sessionUpdate"))
                .and_then(serde_json::Value::as_str);
            if matches!(kind, Some("tool_call" | "tool_call_update"))
                && let (Some(session_id), Some(tool_call_id)) = (
                    params.get("sessionId").and_then(serde_json::Value::as_str),
                    update
                        .and_then(|update| update.get("toolCallId"))
                        .and_then(serde_json::Value::as_str),
                )
            {
                let observation = if kind == Some("tool_call") {
                    ToolObservation::Announcement
                } else {
                    ToolObservation::Continuation
                };
                if !self.0.observe_tool(session_id, tool_call_id, observation) {
                    return Ok(Handled::Yes);
                }
            }
        }
        Ok(Handled::No {
            message,
            retry: false,
        })
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "ToolCallOwnershipHandler"
    }
}

#[cfg(test)]
#[path = "provider_connection_activity_test.rs"]
mod tests;
