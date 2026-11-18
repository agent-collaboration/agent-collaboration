//! The seam between Router tools and the link (frozen 2026-11-17, see
//! `docs/specs/2026-11-17-link-and-router-retrofit/seam.html`). A tool submits a
//! caller's payload; the operation's kind, never the caller, picks the path.
use central_link_protocol::{ProtocolError, ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use std::{future::Future, pin::Pin};
use workspace_model::{Caller, RequestId};

/// An outbox receipt is identified by the write's caller-minted UUIDv7 RequestId.
pub type OutboxReceiptId = RequestId;

/// Boxed so tools can hold `Arc<dyn LinkCalls>`; the seam's calls are async.
pub type LinkCallFuture<'call, TOutput> = Pin<Box<dyn Future<Output = TOutput> + Send + 'call>>;

/// What a Router tool calls. Async, driven by one injected clock.
pub trait LinkCalls: Send + Sync {
    /// Every write payload carries its caller-minted UUIDv7 RequestId.
    fn submit(&self, caller: Caller, payload: RequestPayload) -> LinkCallFuture<'_, Submission>;

    /// Scoped to the caller's own requests: another caller's id answers NotFound.
    fn request_status(
        &self,
        caller: Caller,
        request: RequestId,
    ) -> LinkCallFuture<'_, RequestStatus>;
}

#[derive(Debug)]
pub enum Submission {
    /// Immediate kinds: central's answer.
    Answered(ReplyPayload),
    /// Outbox kinds: stored, sends on the next mark.
    InOutbox(OutboxReceipt),
    /// Nothing sent, nothing stored.
    Refused(SubmitRefusal),
    /// Immediate kinds: sent, answer lost; it may have applied. Reconcile with
    /// `request_status` or resubmit the same id; never retry with a new id.
    Uncertain { request: RequestId },
    /// Reads only, no id: the read frame left and the answer was lost.
    /// A read changes nothing, so reading again is safe.
    ReadAnswerLost,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxReceipt {
    pub id: OutboxReceiptId,
    /// The next 1-minute mark.
    pub flush_at: SyncTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestStatus {
    NotFound,
    InOutbox {
        flush_at: SyncTime,
    },
    /// Flushed; central's answer not yet in.
    Sent,
    /// Central's whole answer, including domain refusals in WorkspaceResult
    /// and CentralMailResult.
    Answered(ReplyPayload),
    /// Central never admitted the frame.
    Refused(LinkAdmissionRefusal),
    /// The Router stopped trying to send it (owner, record:358).
    Cancelled {
        reason: OutboxCancellation,
    },
}

/// Why the Router stopped trying to send an outbox write (owner, record:358).
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutboxCancellation {
    /// A direct message that never left the Mac within 15 minutes of when the
    /// agent sent it: "15 minutes you can't send in central or machine then
    /// tell the sender and cancel send".
    NotDeliveredWithin15Minutes,
    /// A direct message that left the Mac but whose answer never came within
    /// 15 minutes: it may have been delivered. Central's DeliveryOutcome, matched
    /// by the request id, later tells the truth (Lead A, 2026-11-18).
    NotConfirmedWithin15Minutes,
    /// Any other outbox write still unanswered 24 hours after it was stored,
    /// inside central's 24-hour request log, so a replay never commits twice.
    GaveUpAfter24Hours,
}

/// Why central never admitted a frame: the link's own admission, not a domain
/// refusal (those arrive inside an answered reply).
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkAdmissionRefusal {
    /// The machine token was refused.
    CredentialRefused,
    /// The enrollment is revoked, superseded or expired.
    MachineRevoked,
    /// The request id was already used for different content.
    RequestIdConflict,
}

#[derive(Debug, thiserror::Error)]
pub enum SubmitRefusal {
    /// Immediate kinds: nothing left the Mac.
    #[error("central is unavailable; nothing left this Mac")]
    CentralUnavailable,
    /// Outbox kinds: the local write failed; nothing stored.
    #[error("the outbox could not store the write; nothing was stored")]
    OutboxUnavailable,
    #[error("this Router is not enrolled")]
    NotEnrolled,
    /// The same RequestId was already used with different content or by
    /// another caller; nothing new was stored or sent.
    #[error("this request id was already used for a different request")]
    RequestIdConflict,
    #[error("invalid request: {0}")]
    InvalidRequest(ProtocolError),
}
