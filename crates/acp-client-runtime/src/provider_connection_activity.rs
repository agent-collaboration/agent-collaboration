//! Track active provider Turns and resolve tool-scoped connection requests.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use provider_session_machines::SessionGeneration;

#[derive(Default)]
pub(crate) struct ProviderConnectionActivity(Mutex<RegistryState>);

/// The turn a session is running now, and the run it belongs to.
#[derive(Clone)]
struct RunningTurn {
    turn_id: String,
    generation: SessionGeneration,
}

/// Who announced a tool call: the session, its run and turn when one was
/// running, and whether that turn has ended. An ended owner stays as a
/// tombstone through the session's next run (review F4), so a late callback
/// for it is dropped rather than resolved to whatever turn runs now.
#[derive(Clone)]
struct ToolOwner {
    session_id: String,
    run: Option<RunningTurn>,
    ended: bool,
}

impl ToolOwner {
    fn owns_turn(&self, turn: &RunningTurn) -> bool {
        !self.ended
            && self.run.as_ref().is_some_and(|claimed| {
                claimed.generation == turn.generation && claimed.turn_id == turn.turn_id
            })
    }
}

/// The session a tool-scoped callback belongs to, and the run it was
/// announced in when known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ToolCallOwner {
    pub(crate) session_id: String,
    pub(crate) generation: Option<SessionGeneration>,
}

#[derive(Default)]
struct RegistryState {
    running_sessions: HashMap<String, RunningTurn>,
    tool_owners: HashMap<String, Vec<ToolOwner>>,
}

impl RegistryState {
    /// The session's turn ended: its tool calls become tombstones.
    fn end_session_turn(&mut self, session_id: &str) {
        self.running_sessions.remove(session_id);
        for owners in self.tool_owners.values_mut() {
            for owner in owners
                .iter_mut()
                .filter(|owner| owner.session_id == session_id)
            {
                owner.ended = true;
            }
        }
    }
}

impl ProviderConnectionActivity {
    pub(crate) fn turn_started(
        &self,
        session_id: &str,
        turn_id: &str,
        generation: SessionGeneration,
    ) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.running_sessions.insert(
            session_id.to_owned(),
            RunningTurn {
                turn_id: turn_id.to_owned(),
                generation,
            },
        );
        // Tombstones last through the session's next run, then go.
        state.tool_owners.retain(|_, owners| {
            owners.retain(|owner| {
                !(owner.session_id == session_id
                    && owner.ended
                    && owner
                        .run
                        .as_ref()
                        .is_none_or(|run| run.generation.number() + 1 < generation.number()))
            });
            !owners.is_empty()
        });
    }

    /// A first announcement can claim an ID for this session. A repeated
    /// announcement carries no turn identity and cannot replace that owner.
    fn announce_tool_call(&self, session_id: &str, tool_call_id: &str) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let run = state.running_sessions.get(session_id).cloned();
        let owners = state
            .tool_owners
            .entry(tool_call_id.to_owned())
            .or_default();
        if let Some(owner) = owners.iter().find(|owner| owner.session_id == session_id) {
            return run.as_ref().is_none_or(|turn| owner.owns_turn(turn));
        }
        owners.push(ToolOwner {
            session_id: session_id.to_owned(),
            run,
            ended: false,
        });
        true
    }

    /// A continuation can't claim ownership. A known ID can update its
    /// original turn, or continue an item established by replay without a
    /// live turn. A retired owner cannot update either projection.
    fn allows_tool_continuation(&self, session_id: &str, tool_call_id: &str) -> bool {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(turn) = state.running_sessions.get(session_id) else {
            return true;
        };
        state.tool_owners.get(tool_call_id).is_none_or(|owners| {
            owners.iter().any(|owner| {
                owner.session_id == session_id
                    && (owner.owns_turn(turn) || (!owner.ended && owner.run.is_none()))
            })
        })
    }

    /// The owner of a tool-scoped callback. An announced tool call resolves
    /// to its one live owner; one whose turn has ended resolves to nothing,
    /// so the callback is dropped. A tool call never announced falls back to
    /// the one running turn (Cursor omits sessionId; the tested contract).
    pub(crate) fn resolve_session(&self, tool_call_id: &str) -> Option<ToolCallOwner> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(owners) = state.tool_owners.get(tool_call_id) {
            let live: Vec<&ToolOwner> = owners.iter().filter(|owner| !owner.ended).collect();
            let sessions: HashSet<&str> =
                live.iter().map(|owner| owner.session_id.as_str()).collect();
            if sessions.len() != 1 {
                return None;
            }
            return live.last().map(|owner| ToolCallOwner {
                session_id: owner.session_id.clone(),
                generation: owner.run.as_ref().map(|run| run.generation),
            });
        }
        if state.running_sessions.len() != 1 {
            return None;
        }
        state
            .running_sessions
            .iter()
            .next()
            .map(|(session_id, run)| ToolCallOwner {
                session_id: session_id.clone(),
                generation: Some(run.generation),
            })
    }

    pub(crate) fn turn_ended(&self, session_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .end_session_turn(session_id);
    }

    pub(crate) fn claim_turn_end(&self, session_id: &str, turn_id: &str) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .running_sessions
            .get(session_id)
            .map(|run| run.turn_id.as_str())
            != Some(turn_id)
        {
            return false;
        }
        state.end_session_turn(session_id);
        true
    }

    pub(crate) fn drain_running_turns(&self) -> Vec<(String, String)> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.tool_owners.clear();
        state
            .running_sessions
            .drain()
            .map(|(session_id, run)| (session_id, run.turn_id))
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
                let accepted = match kind {
                    Some("tool_call") => self.0.announce_tool_call(session_id, tool_call_id),
                    Some("tool_call_update") => {
                        self.0.allows_tool_continuation(session_id, tool_call_id)
                    }
                    _ => true,
                };
                if !accepted {
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
