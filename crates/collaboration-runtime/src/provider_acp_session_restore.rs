//! Connection-owned restore requests and the frames that arrive during them.

use codex_acp_adapter::QueuedAcpFrame;
use collaboration_contracts::{SessionCommandPort, SessionEventHub, SessionTargetCommand};
use message_board::{Identity, SessionRef};
use router_clock::RuntimeClock;
use serde_json::Value;
use session_event_model::{CapabilityReport, SessionState};
use std::{collections::VecDeque, time::Duration};

use super::{command_failure, failure, response, session_result};

const STORE_BUDGET: Duration = Duration::from_secs(10);
const STORE_ELAPSED: (i32, &str) = (-32000, "the store didn't respond in time");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AcpRestoreMethod {
    Load,
    Resume,
}

impl AcpRestoreMethod {
    pub(super) fn from_rpc(method: &str) -> Option<Self> {
        match method {
            "session/load" => Some(Self::Load),
            "session/resume" => Some(Self::Resume),
            _ => None,
        }
    }
}

pub(super) const RESTORE_IN_PROGRESS: (i32, &str) = (
    -32000,
    "session was loading when this request arrived; check the first restore reply before retrying",
);

pub(super) enum WaitingSessionFrame {
    Command(QueuedAcpFrame),
    DuplicateLoad {
        frame: QueuedAcpFrame,
        session: SessionRef,
        cwd: String,
    },
    RestoreRefusal {
        frame: QueuedAcpFrame,
        code: i32,
        message: &'static str,
    },
}

/// Entered when the route starts restore validation. Released only after the
/// reply is queued, so later frames run in arrival order. Keeping their
/// input frames also keeps the connection's existing byte-budget permits.
#[derive(Default)]
pub(super) struct AwaitingRestore {
    pub(super) frames: VecDeque<WaitingSessionFrame>,
}

pub(super) struct RestoreRequest {
    pub(super) method: AcpRestoreMethod,
    pub(super) session: SessionRef,
    pub(super) id: Value,
    pub(super) cwd: Option<String>,
}

pub(super) enum RestoreCompletion {
    Activated(RestoreRequest),
    Reply { session: SessionRef, reply: Value },
}

impl RestoreRequest {
    pub(super) async fn activate(
        self,
        commands: &dyn SessionCommandPort,
        events: &dyn SessionEventHub,
        clock: &dyn RuntimeClock,
        actor: Identity,
    ) -> RestoreCompletion {
        if let Some(cwd) = self.cwd.as_deref()
            && let Err((code, message)) =
                validate_load_inventory(events, clock, &self.session, cwd).await
        {
            return RestoreCompletion::Reply {
                session: self.session,
                reply: failure(self.id, code, message),
            };
        }
        match router_clock::timeout(clock, STORE_BUDGET, events.state(self.session.clone())).await {
            Ok(Ok(SessionState::Unloaded)) => {}
            Ok(Ok(_)) => return RestoreCompletion::Activated(self),
            Ok(Err(_)) => {
                return RestoreCompletion::Reply {
                    session: self.session,
                    reply: failure(self.id, -32002, "Session not found"),
                };
            }
            Err(_) => {
                return RestoreCompletion::Reply {
                    session: self.session,
                    reply: failure(self.id, STORE_ELAPSED.0, STORE_ELAPSED.1),
                };
            }
        }
        let command = SessionTargetCommand {
            target: self.session.clone(),
            actor,
        };
        let outcome = match self.method {
            AcpRestoreMethod::Load => commands.load_session(command).await,
            AcpRestoreMethod::Resume => commands.resume_session(command).await,
        };
        match outcome {
            Ok(()) => RestoreCompletion::Activated(self),
            Err(error) => RestoreCompletion::Reply {
                session: self.session,
                reply: command_failure(self.id, error),
            },
        }
    }

    pub(super) async fn reply(
        self,
        events: &dyn SessionEventHub,
        clock: &dyn RuntimeClock,
        capabilities: CapabilityReport,
    ) -> RestoreCompletion {
        let history_unavailable = self.method == AcpRestoreMethod::Resume;
        let result = router_clock::timeout(
            clock,
            STORE_BUDGET,
            session_result(events, &self.session, capabilities, history_unavailable),
        )
        .await;
        let reply = match result {
            Ok(Ok(payload)) => response(self.id, payload),
            Ok(Err(())) => failure(self.id, -32000, "Session inventory unavailable"),
            Err(_) => failure(self.id, STORE_ELAPSED.0, STORE_ELAPSED.1),
        };
        RestoreCompletion::Reply {
            session: self.session,
            reply,
        }
    }
}

pub(super) async fn reject_duplicate_load(
    events: &dyn SessionEventHub,
    clock: &dyn RuntimeClock,
    session: SessionRef,
    id: Value,
    cwd: String,
) -> RestoreCompletion {
    let (code, message) = validate_load_inventory(events, clock, &session, &cwd)
        .await
        .err()
        .unwrap_or(RESTORE_IN_PROGRESS);
    RestoreCompletion::Reply {
        session,
        reply: failure(id, code, message),
    }
}

async fn validate_load_inventory(
    events: &dyn SessionEventHub,
    clock: &dyn RuntimeClock,
    session: &SessionRef,
    cwd: &str,
) -> Result<(), (i32, &'static str)> {
    let summaries = router_clock::timeout(
        clock,
        STORE_BUDGET,
        events.sessions(session.endpoint.clone()),
    )
    .await
    .map_err(|_| STORE_ELAPSED)?
    .map_err(|_| (-32000, "Session inventory unavailable"))?;
    let stored = summaries
        .into_iter()
        .find(|summary| summary.session == *session)
        .ok_or((-32002, "Session not found"))?;
    if stored.working_directory.to_str() != Some(cwd) {
        return Err((-32602, "Session working directory does not match"));
    }
    Ok(())
}
