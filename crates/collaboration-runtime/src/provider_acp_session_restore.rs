//! Connection-owned restore requests and the frames that arrive during them.

use codex_acp_adapter::QueuedAcpFrame;
use collaboration_contracts::{CommandFailure, SessionCommandPort, SessionTargetCommand};
use message_board::{Identity, SessionRef};
use serde_json::Value;
use std::collections::VecDeque;

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
    RestoreRefusal {
        frame: QueuedAcpFrame,
        code: i32,
        message: &'static str,
    },
}

/// Entered when the route starts a Host restore. Released only after its reply
/// is queued, so later frames can then run in arrival order. Keeping their
/// input frames also keeps the connection's existing byte-budget permits.
#[derive(Default)]
pub(super) struct AwaitingRestore {
    pub(super) frames: VecDeque<WaitingSessionFrame>,
}

pub(super) struct RestoreRequest {
    pub(super) method: AcpRestoreMethod,
    pub(super) session: SessionRef,
    pub(super) id: Value,
}

pub(super) struct RestoreCompletion {
    pub(super) request: RestoreRequest,
    pub(super) outcome: Result<(), CommandFailure>,
}

impl RestoreRequest {
    pub(super) async fn activate(
        self,
        commands: &dyn SessionCommandPort,
        actor: Identity,
    ) -> RestoreCompletion {
        let command = SessionTargetCommand {
            target: self.session.clone(),
            actor,
        };
        let outcome = match self.method {
            AcpRestoreMethod::Load => commands.load_session(command).await,
            AcpRestoreMethod::Resume => commands.resume_session(command).await,
        };
        RestoreCompletion {
            request: self,
            outcome,
        }
    }
}
