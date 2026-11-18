//! When an outbox write stops being sendable, and how it ends then. One owner
//! for both stores (SQLite and memory) so they cannot disagree.
//!
//! Every outbox write, direct message or thread post, has 15 minutes from
//! when the agent wrote it, which is the row's creation stamp (owner,
//! record:218 and 2026-11-18: "psots shoudl get 15min too in teh outlbox").
//! That is well inside central's 24-hour request log, so a replay before the
//! deadline is always answered from the log, never committed twice.

pub(crate) const OUTBOX_WINDOW_MILLIS: i64 = 15 * 60 * 1000;

/// The first instant a write stored at `created_at_ms` may no longer be sent.
pub(crate) fn expires_at_ms(created_at_ms: i64) -> i64 {
    created_at_ms.saturating_add(OUTBOX_WINDOW_MILLIS)
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
    departure: DepartureAtDeadline,
) -> central_link_client::OutboxCancellation {
    match departure {
        DepartureAtDeadline::NeverLeft => {
            central_link_client::OutboxCancellation::NotSentWithin15Minutes
        }
        DepartureAtDeadline::MayHaveArrived => {
            central_link_client::OutboxCancellation::NotConfirmedWithin15Minutes
        }
    }
}

#[cfg(test)]
#[path = "outbox_deadline_test.rs"]
mod tests;
