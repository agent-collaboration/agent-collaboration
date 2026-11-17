//! One queued message in a session's line, as a pure machine (D1 state
//! machines; design note §12.2, §13.1, §13.2, §15.1).
//!
//! The durable dispatch mark is a precondition; the handover is the single
//! Marking → Sent move, taken only while the Router's transport is open. Until
//! then a close, cancel or shutdown settles the entry "not sent". From the
//! runner's claim (Marking) the runner owns the one settlement; from Sent the
//! request's completion task owns it. Every path ends in exactly one Done.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

/// Why a queued message wasn't sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NotSentReason {
    SessionClosed,
    CancelledFromQueue,
    RouterShuttingDown,
    ProviderConnectionClosed,
    DispatchNotRecorded,
    DispatchRecordUncertain,
    /// The session's actor exited while the message waited.
    AgentStopped,
    /// The load the message waited for failed.
    LoadFailed,
}

impl NotSentReason {
    pub(crate) const fn human_phrase(self) -> &'static str {
        match self {
            Self::SessionClosed => "session closed",
            Self::CancelledFromQueue => "cancelled from the queue",
            Self::RouterShuttingDown => "Router shutting down",
            Self::ProviderConnectionClosed => "provider connection closed",
            Self::DispatchNotRecorded => "the dispatch couldn't be recorded",
            Self::DispatchRecordUncertain => "dispatch record uncertain; not sent",
            Self::AgentStopped => "the agent stopped",
            Self::LoadFailed => "the session couldn't be loaded",
        }
    }
}

/// How a sent message's turn ended, as its completion task read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SentOutcome {
    Applied,
    /// The agent refused with a definite code: not applied (design note §16).
    Refused,
    /// Possibly dispatched: Lost, an internal error or a cancelled request.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryEnding {
    Sent(SentOutcome),
    NotSent(NotSentReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedEntry {
    Waiting,
    Held,
    /// Committed to send; the runner hasn't claimed it yet.
    Committed,
    /// The runner claimed it and is writing the durable dispatch mark.
    Marking,
    /// A close, cancel or shutdown won while the mark was being written; the
    /// runner undoes the mark and settles.
    Withdrawing(NotSentReason),
    /// Handed over: the request is enqueued and its completion task owns it.
    Sent,
    Done(EntryEnding),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedEntryEvent {
    Claimed,
    Commit,
    StartSendClaimed,
    /// The mark committed; the handover re-checks the transport and the run.
    MarkCommitted {
        transport_open: bool,
        run_live: bool,
    },
    MarkFailed,
    MarkUncertain,
    Withdraw(NotSentReason),
    Cancel,
    UnmarkCommitted,
    UnmarkFailed,
    TurnSettled(SentOutcome),
}

/// The runner's work after a move (design note §11.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedEntryEffect {
    StartSend,
    WriteMark,
    /// The handover: send the request and start its completion task.
    SendRequest,
    Unmark,
    SettleNotSent(NotSentReason),
    /// The mark's outcome is unknown: read the durable row back and settle by it.
    ReadBackRow(NotSentReason),
    /// The unmark failed: not sent, while the durable row still says
    /// may-have-dispatched.
    OverlayNotSentOverDispatch(NotSentReason),
    SettleSent(SentOutcome),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueuedEntryRefusal {
    /// Cancel on a message already handed over: cancel the turn instead.
    AlreadySent,
    /// The entry already has its ending.
    Settled,
}

impl QueuedEntryRefusal {
    pub(crate) const fn human_phrase(self) -> &'static str {
        match self {
            Self::AlreadySent => "already sent; cancel the turn instead",
            Self::Settled => "the message already ended",
        }
    }
}

impl RefusalTag for QueuedEntryRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadySent => "already_sent",
            Self::Settled => "settled",
        }
    }
}

impl StateTag for QueuedEntry {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Held => "held",
            Self::Committed => "committed",
            Self::Marking => "marking",
            Self::Withdrawing(_) => "withdrawing",
            Self::Sent => "sent",
            Self::Done(EntryEnding::Sent(_)) => "done_sent",
            Self::Done(EntryEnding::NotSent(_)) => "done_not_sent",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Done(_) => true,
            Self::Waiting
            | Self::Held
            | Self::Committed
            | Self::Marking
            | Self::Withdrawing(_)
            | Self::Sent => false,
        }
    }
}

impl EventTag for QueuedEntryEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::Commit => "commit",
            Self::StartSendClaimed => "start_send_claimed",
            Self::MarkCommitted { .. } => "mark_committed",
            Self::MarkFailed => "mark_failed",
            Self::MarkUncertain => "mark_uncertain",
            Self::Withdraw(_) => "withdraw",
            Self::Cancel => "cancel",
            Self::UnmarkCommitted => "unmark_committed",
            Self::UnmarkFailed => "unmark_failed",
            Self::TurnSettled(_) => "turn_settled",
        }
    }
}

const fn edge(
    from: &'static str,
    event: &'static str,
    to: &'static str,
    terminal: bool,
) -> Transition {
    Transition {
        from,
        event,
        to,
        terminal,
    }
}

const TRANSITIONS: &[Transition] = &[
    edge("waiting", "claimed", "held", false),
    edge("held", "commit", "committed", false),
    edge("committed", "start_send_claimed", "marking", false),
    edge("marking", "mark_committed", "sent", false),
    edge("marking", "mark_committed", "withdrawing", false),
    edge("marking", "mark_failed", "done_not_sent", true),
    edge("marking", "mark_uncertain", "done_not_sent", true),
    edge("waiting", "withdraw", "done_not_sent", true),
    edge("held", "withdraw", "done_not_sent", true),
    edge("committed", "withdraw", "done_not_sent", true),
    edge("marking", "withdraw", "withdrawing", false),
    edge("waiting", "cancel", "done_not_sent", true),
    edge("held", "cancel", "done_not_sent", true),
    edge("committed", "cancel", "done_not_sent", true),
    edge("marking", "cancel", "withdrawing", false),
    edge("withdrawing", "mark_committed", "withdrawing", false),
    edge("withdrawing", "mark_failed", "done_not_sent", true),
    edge("withdrawing", "mark_uncertain", "done_not_sent", true),
    edge("withdrawing", "unmark_committed", "done_not_sent", true),
    edge("withdrawing", "unmark_failed", "done_not_sent", true),
    edge("sent", "turn_settled", "done_sent", true),
];

pub(crate) struct QueuedEntryMachine;

fn not_sent(reason: NotSentReason) -> (QueuedEntry, Vec<QueuedEntryEffect>) {
    (
        QueuedEntry::Done(EntryEnding::NotSent(reason)),
        vec![QueuedEntryEffect::SettleNotSent(reason)],
    )
}

impl StateMachine for QueuedEntryMachine {
    type State = QueuedEntry;
    type Event = QueuedEntryEvent;
    type Effect = QueuedEntryEffect;
    type Error = QueuedEntryRefusal;

    fn transition(state: &QueuedEntry, event: QueuedEntryEvent) -> StateMachineResult<Self> {
        use QueuedEntry as S;
        use QueuedEntryEffect as E;
        use QueuedEntryEvent as V;
        match (*state, event) {
            (S::Waiting, V::Claimed) => Ok((S::Held, Vec::new())),
            (S::Held, V::Commit) => Ok((S::Committed, vec![E::StartSend])),
            (S::Committed, V::StartSendClaimed) => Ok((S::Marking, vec![E::WriteMark])),
            (
                S::Marking,
                V::MarkCommitted {
                    transport_open: true,
                    run_live: true,
                },
            ) => Ok((S::Sent, vec![E::SendRequest])),
            (
                S::Marking,
                V::MarkCommitted {
                    transport_open: false,
                    ..
                },
            ) => Ok((
                S::Withdrawing(NotSentReason::ProviderConnectionClosed),
                vec![E::Unmark],
            )),
            (
                S::Marking,
                V::MarkCommitted {
                    transport_open: true,
                    run_live: false,
                },
            ) => Ok((
                S::Withdrawing(NotSentReason::SessionClosed),
                vec![E::Unmark],
            )),
            (S::Marking, V::MarkFailed) => Ok(not_sent(NotSentReason::DispatchNotRecorded)),
            (S::Marking, V::MarkUncertain) => Ok((
                S::Done(EntryEnding::NotSent(NotSentReason::DispatchRecordUncertain)),
                vec![E::ReadBackRow(NotSentReason::DispatchRecordUncertain)],
            )),
            (S::Waiting | S::Held | S::Committed, V::Withdraw(reason)) => Ok(not_sent(reason)),
            (S::Waiting | S::Held | S::Committed, V::Cancel) => {
                Ok(not_sent(NotSentReason::CancelledFromQueue))
            }
            (S::Marking, V::Withdraw(reason)) => Ok((S::Withdrawing(reason), Vec::new())),
            (S::Marking, V::Cancel) => Ok((
                S::Withdrawing(NotSentReason::CancelledFromQueue),
                Vec::new(),
            )),
            (S::Withdrawing(reason), V::MarkCommitted { .. }) => {
                Ok((S::Withdrawing(reason), vec![E::Unmark]))
            }
            (S::Withdrawing(reason), V::MarkFailed | V::UnmarkCommitted) => Ok(not_sent(reason)),
            (S::Withdrawing(reason), V::MarkUncertain) => Ok((
                S::Done(EntryEnding::NotSent(reason)),
                vec![E::ReadBackRow(reason)],
            )),
            (S::Withdrawing(reason), V::UnmarkFailed) => Ok((
                S::Done(EntryEnding::NotSent(reason)),
                vec![E::OverlayNotSentOverDispatch(reason)],
            )),
            (S::Sent, V::TurnSettled(outcome)) => Ok((
                S::Done(EntryEnding::Sent(outcome)),
                vec![E::SettleSent(outcome)],
            )),
            (S::Sent, V::Cancel) => Err(TransitionError::Domain(QueuedEntryRefusal::AlreadySent)),
            // A close or shutdown reaching a sent message changes nothing
            // here: the session cancels the turn and the completion settles.
            (S::Sent, V::Withdraw(_)) => Err(TransitionError::Stale),
            (
                S::Done(_),
                V::Claimed
                | V::Commit
                | V::StartSendClaimed
                | V::MarkCommitted { .. }
                | V::MarkFailed
                | V::MarkUncertain
                | V::Withdraw(_)
                | V::Cancel
                | V::UnmarkCommitted
                | V::UnmarkFailed
                | V::TurnSettled(_),
            ) => Err(TransitionError::Domain(QueuedEntryRefusal::Settled)),
            (
                S::Waiting | S::Held | S::Committed | S::Marking | S::Withdrawing(_) | S::Sent,
                V::Claimed
                | V::Commit
                | V::StartSendClaimed
                | V::MarkCommitted { .. }
                | V::MarkFailed
                | V::MarkUncertain
                | V::Withdraw(_)
                | V::Cancel
                | V::UnmarkCommitted
                | V::UnmarkFailed
                | V::TurnSettled(_),
            ) => Err(TransitionError::NotAllowed {
                state: state.state_tag(),
                event: event.event_tag(),
            }),
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "queued_entry_machine_test.rs"]
mod tests;
