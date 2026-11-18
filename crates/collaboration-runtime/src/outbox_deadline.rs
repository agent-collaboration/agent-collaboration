//! When an outbox write stops being sendable, and how it ends then. One owner
//! for both stores (SQLite and memory) so they cannot disagree.
//!
//! A direct message has 15 minutes from when the agent sent it, which is the
//! row's creation stamp (owner, record:218; design 2026-11-17). Every other
//! write has 24 hours, the life of central's request log: a write replayed
//! inside it is answered from the log, never committed twice (step 9).
use central_link_client::{OutboxCancellation, OutboxWriteTarget};

pub(crate) const DIRECT_MESSAGE_WINDOW_MILLIS: i64 = 15 * 60 * 1000;
pub(crate) const OTHER_WRITE_WINDOW_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// The first instant a write stored at `created_at_ms` may no longer be sent.
pub(crate) fn expires_at_ms(target: &OutboxWriteTarget, created_at_ms: i64) -> i64 {
    let window = match target {
        OutboxWriteTarget::Session { .. } => DIRECT_MESSAGE_WINDOW_MILLIS,
        OutboxWriteTarget::Thread { .. } => OTHER_WRITE_WINDOW_MILLIS,
    };
    created_at_ms.saturating_add(window)
}

/// Whether an overdue write had already left the Mac, so central may hold it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DepartureAtDeadline {
    /// Never sent, or only a first attempt that central did not apply.
    NeverLeft,
    /// Sent with no answer back (on the wire, or the answer was lost).
    MayHaveArrived,
}

/// How an overdue write ends.
pub(crate) fn end_reason(
    target: &OutboxWriteTarget,
    departure: DepartureAtDeadline,
) -> OutboxCancellation {
    match (target, departure) {
        (OutboxWriteTarget::Session { .. }, DepartureAtDeadline::NeverLeft) => {
            OutboxCancellation::NotDeliveredWithin15Minutes
        }
        (OutboxWriteTarget::Session { .. }, DepartureAtDeadline::MayHaveArrived) => {
            OutboxCancellation::NotConfirmedWithin15Minutes
        }
        (OutboxWriteTarget::Thread { .. }, DepartureAtDeadline::NeverLeft) => {
            OutboxCancellation::NotSentWithin24Hours
        }
        (OutboxWriteTarget::Thread { .. }, DepartureAtDeadline::MayHaveArrived) => {
            OutboxCancellation::NotConfirmedWithin24Hours
        }
    }
}

#[cfg(test)]
#[path = "outbox_deadline_test.rs"]
mod tests;
