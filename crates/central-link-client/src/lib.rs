//! Event-driven client policy. The Host owns socket I/O and drives the injected-clock deadline.
//! Below the tools' seam: `LinkCalls`, the outbox kind rule and the flush reducer.
mod error;
mod flush_marks;
mod link_calls;
mod link_state;
mod outbox_flush;
mod outbox_views;
mod pending_calls;
mod submission_path;

#[cfg(any(test, feature = "test-support"))]
mod fake_link_calls;

pub use flush_marks::{
    FLUSH_INTERVAL_MILLIS, FLUSH_JITTER_MILLIS, earliest_open_mark, mark_in_window, next_mark_after,
};
pub use link_calls::{
    LinkAdmissionRefusal, LinkCallFuture, LinkCalls, OutboxCancellation, OutboxReceipt,
    OutboxReceiptId, RequestStatus, Submission, SubmitRefusal,
};
pub use link_state::{
    ClientAction, ClientEvent, ConnectionAttempt, LinkClient, LinkStatus, RefusalReason,
};
pub use outbox_flush::{
    FlushEffect, FlushEvent, FlushTransitionError, LinkAvailability, MAX_BATCH_WRITES,
    MemberOutcome, OutboxFlushState, transition,
};
pub use outbox_views::{
    EndedWrite, FlushOutcome, FlushOutcomeSink, FlushOutcomes, OutboxWriteContent,
    OutboxWriteTarget, OutgoingOutcome, OutgoingWrite, PendingWrite, PendingWriteState,
};
pub use pending_calls::{CallFailure, FailedCall, SendFailure};
pub use submission_path::{SubmissionPath, submission_path};

#[cfg(any(test, feature = "test-support"))]
pub use fake_link_calls::{FakeCentral, FakeLinkCalls, FakeResponder};

pub use error::ClientError;
