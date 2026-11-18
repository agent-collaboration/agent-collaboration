//! What a caller sees of its own outbox writes, below the tools' seam: the
//! unsent and unresolved ones (`pending_for`), the recently answered ones
//! (`outgoing_for`), and each flush's outcomes for per-session receipts.
use crate::outbox_flush::MemberOutcome;
use central_link_protocol::{ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use workspace_model::{
    Caller, PostText, RequestId, SessionAddress, ThreadId, WorkspaceId, WorkspaceOperation,
    central_mail::CentralMailOperation,
};

/// Where an outbox write goes: a thread post or a direct message. It holds no
/// text, so the Mac may keep it after central answers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OutboxWriteTarget {
    Thread {
        workspace_id: Option<WorkspaceId>,
        thread_id: ThreadId,
    },
    Session {
        to: SessionAddress,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxWriteContent {
    pub target: OutboxWriteTarget,
    pub body: PostText,
}

impl OutboxWriteContent {
    /// The outbox only holds `ThreadPost` and `SendMessage`; any other payload
    /// answers None and is not an outbox write.
    #[must_use]
    pub fn of(payload: &RequestPayload) -> Option<Self> {
        match payload {
            RequestPayload::Workspace(request) => match &request.operation {
                WorkspaceOperation::ThreadPost {
                    thread_id, body, ..
                } => Some(Self {
                    target: OutboxWriteTarget::Thread {
                        workspace_id: request.workspace_id.clone(),
                        thread_id: thread_id.clone(),
                    },
                    body: body.clone(),
                }),
                _ => None,
            },
            RequestPayload::Mail(request) => match &request.operation {
                CentralMailOperation::SendMessage { to, body } => Some(Self {
                    target: OutboxWriteTarget::Session { to: to.clone() },
                    body: body.clone(),
                }),
                _ => None,
            },
        }
    }
}

/// Where a pending write stands. A write that went out in a batch whose answer
/// was lost may already be in central; it is resent under the same id. Its text
/// stays on the Mac only until central answers (owner, 2026-11-15: "unsent text
/// in the Mac outbox: allowed until central answers"), so only the unanswered
/// states carry the body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingWriteState {
    /// Never left the Mac.
    Unsent { body: PostText },
    /// Went out in a batch whose answer was lost; resent with the same id.
    SentAnswerLost { body: PostText },
    /// Central answered; the read layer drops it once it sees the created id.
    Answered(ReplyPayload),
    /// The Router stopped trying to send it; its text is gone.
    Cancelled {
        reason: crate::link_calls::OutboxCancellation,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingWrite {
    pub request_id: RequestId,
    pub target: OutboxWriteTarget,
    pub flush_at: SyncTime,
    pub state: PendingWriteState,
}

/// How central answered a flushed write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutgoingOutcome {
    Answered(ReplyPayload),
    Refused(crate::link_calls::LinkAdmissionRefusal),
}

/// An answered write, without its text: central holds the post or message now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutgoingWrite {
    pub request_id: RequestId,
    pub target: OutboxWriteTarget,
    pub outcome: OutgoingOutcome,
    pub answered_at: SyncTime,
}

/// One member's outcome from a flush, attributed to its caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlushOutcome {
    pub caller: Caller,
    pub request_id: RequestId,
    pub outcome: MemberOutcome,
}

/// The outcomes of one flush. `complete` is false when some members went back
/// to the outbox because the batch ended without their answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlushOutcomes {
    pub flush_at: SyncTime,
    pub outcomes: Vec<FlushOutcome>,
    pub complete: bool,
}

/// Receives every flush's outcomes; the CLI+MCP side groups and words them per
/// session.
pub trait FlushOutcomeSink: Send + Sync {
    fn deliver(&self, outcomes: FlushOutcomes);
}

#[cfg(test)]
#[path = "outbox_views_test.rs"]
mod tests;
