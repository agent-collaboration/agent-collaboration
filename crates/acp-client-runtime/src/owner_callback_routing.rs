//! Route ACP callbacks before a later owner command can change their target.

use crate::{
    acp_transport::{CallbackKind, CallbackResolution, TransportRun},
    owner_wire::OwnerAcpWire,
};
use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use provider_session_machines::{
    session_turn_machine::TurnId,
    tool_ownership::{ToolObservation, ToolOwnershipEffect},
};
use std::sync::Arc;

pub(crate) struct ToolCallOwner {
    pub(crate) session_id: String,
    pub(crate) run: TransportRun,
    pub(crate) turn: TurnId,
}

pub(crate) struct OwnerCallbackRouting {
    wire: Arc<OwnerAcpWire>,
}

impl OwnerCallbackRouting {
    pub(crate) fn new(wire: Arc<OwnerAcpWire>) -> Self {
        Self { wire }
    }

    pub(crate) fn resolve_session(&self, tool: &str) -> Option<ToolCallOwner> {
        let (run, turn) = match self.wire.resolve_callback(tool, CallbackKind::Request) {
            CallbackResolution::Exact { run, turn } => (run, turn),
            CallbackResolution::NeverObserved {
                sole_eligible: Some(candidate),
            } => candidate,
            _ => return None,
        };
        Some(ToolCallOwner {
            session_id: self.wire.session_for_route(run)?,
            run,
            turn,
        })
    }

    pub(crate) fn allows_session_permission(&self, session: &str, tool: &str) -> bool {
        self.wire.allows_explicit_tool_request(session, tool)
    }

    pub(crate) fn current_scope(&self, session: &str) -> Option<(TransportRun, TurnId)> {
        let run = self.wire.route_for_session(session)?;
        Some((run, self.wire.current_turn(run)?))
    }

    pub(crate) fn in_folder_grant(
        &self,
        session: &str,
    ) -> Option<crate::in_folder_allowance::InFolderGrant> {
        let run = self.wire.route_for_session(session)?;
        self.wire.in_folder_grant(run)
    }
}

pub(crate) struct OwnerToolCallHandler(Arc<OwnerCallbackRouting>);

impl OwnerToolCallHandler {
    pub(crate) fn new(routing: Arc<OwnerCallbackRouting>) -> Self {
        Self(routing)
    }
}

impl HandleDispatchFrom<Agent> for OwnerToolCallHandler {
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
                && let (Some(session), Some(tool)) = (
                    params.get("sessionId").and_then(serde_json::Value::as_str),
                    update
                        .and_then(|update| update.get("toolCallId"))
                        .and_then(serde_json::Value::as_str),
                )
            {
                let observed = if kind == Some("tool_call") {
                    ToolObservation::Announcement
                } else {
                    ToolObservation::Continuation
                };
                let effect = self
                    .0
                    .wire
                    .route_for_session(session)
                    .map_or(ToolOwnershipEffect::None, |run| {
                        self.0.wire.observe_tool(run, tool, observed)
                    });
                if !matches!(
                    effect,
                    ToolOwnershipEffect::ForwardFrame { .. }
                        | ToolOwnershipEffect::CheckRetainedItem
                ) {
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
        "OwnerToolCallHandler"
    }
}
