//! A session's queue line, as a pure machine (D1 state machines; design note
//! §11.2, §12.2). The line's epoch is its run's generation: a fresh load opens
//! a fresh line, a failed close reopens it, and an entry from an older epoch
//! is Stale. The entries themselves are [`crate::queued_entry_machine`].

use crate::SessionGeneration;
use crate::queued_entry_machine::NotSentReason;
use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueLine {
    Open {
        epoch: SessionGeneration,
        next_sequence: u64,
    },
    /// Keeps its sequence counter, so a reopened line never reuses one.
    Ended {
        epoch: SessionGeneration,
        reason: NotSentReason,
        next_sequence: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueLineEvent {
    Enqueue { epoch: SessionGeneration },
    End { reason: NotSentReason },
    Reopen { epoch: SessionGeneration },
    OpenFresh { epoch: SessionGeneration },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueLineEffect {
    /// A new entry, Waiting, at this sequence.
    Admit { sequence: u64 },
    /// Every Waiting, Held or Committed entry ends not sent, and a Marking one
    /// is withdrawn (its runner settles it).
    WithdrawAll { reason: NotSentReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueLineRefusal {
    /// The line has ended; the caller loads or resumes the session and sends
    /// again (M8).
    Ended(NotSentReason),
}

impl QueueLineRefusal {
    /// The refusal's answer with the caller's next step (M8).
    pub(crate) fn human_phrase(self) -> String {
        match self {
            Self::Ended(reason) => format!(
                "not sent: the session's queue has ended ({}); load or resume the session and send again",
                reason.human_phrase()
            ),
        }
    }
}

impl RefusalTag for QueueLineRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::Ended(_) => "queue_ended",
        }
    }
}

impl StateTag for QueueLine {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Open { .. } => "open",
            Self::Ended { .. } => "ended",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Open { .. } | Self::Ended { .. } => false,
        }
    }
}

impl EventTag for QueueLineEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Enqueue { .. } => "enqueue",
            Self::End { .. } => "end",
            Self::Reopen { .. } => "reopen",
            Self::OpenFresh { .. } => "open_fresh",
        }
    }
}

const fn edge(from: &'static str, event: &'static str, to: &'static str) -> Transition {
    Transition {
        from,
        event,
        to,
        terminal: false,
    }
}

const TRANSITIONS: &[Transition] = &[
    edge("open", "enqueue", "open"),
    edge("open", "end", "ended"),
    edge("ended", "reopen", "open"),
    edge("ended", "open_fresh", "open"),
    edge("open", "open_fresh", "open"),
];

pub(crate) struct QueueLineMachine;

impl StateMachine for QueueLineMachine {
    type State = QueueLine;
    type Event = QueueLineEvent;
    type Effect = QueueLineEffect;
    type Error = QueueLineRefusal;

    fn transition(state: &QueueLine, event: QueueLineEvent) -> StateMachineResult<Self> {
        use QueueLine as S;
        use QueueLineEvent as V;
        match (*state, event) {
            (
                S::Open {
                    epoch,
                    next_sequence,
                },
                V::Enqueue { epoch: from },
            ) if from == epoch => Ok((
                S::Open {
                    epoch,
                    next_sequence: next_sequence.wrapping_add(1),
                },
                vec![QueueLineEffect::Admit {
                    sequence: next_sequence,
                }],
            )),
            (S::Ended { epoch, reason, .. }, V::Enqueue { epoch: from }) if from == epoch => {
                Err(TransitionError::Domain(QueueLineRefusal::Ended(reason)))
            }
            (S::Open { .. } | S::Ended { .. }, V::Enqueue { .. }) => Err(TransitionError::Stale),
            (
                S::Open {
                    epoch,
                    next_sequence,
                },
                V::End { reason },
            ) => Ok((
                S::Ended {
                    epoch,
                    reason,
                    next_sequence,
                },
                vec![QueueLineEffect::WithdrawAll { reason }],
            )),
            (S::Ended { .. }, V::End { .. }) => Err(TransitionError::Stale),
            (
                S::Ended {
                    epoch,
                    next_sequence,
                    ..
                },
                V::Reopen { epoch: from },
            ) if from == epoch => Ok((
                S::Open {
                    epoch,
                    next_sequence,
                },
                Vec::new(),
            )),
            (S::Open { .. } | S::Ended { .. }, V::Reopen { .. }) => Err(TransitionError::Stale),
            // A fresh run continues the session's numbering, so a number held
            // from an earlier run never names a message in this one.
            (
                S::Open {
                    epoch: old,
                    next_sequence,
                }
                | S::Ended {
                    epoch: old,
                    next_sequence,
                    ..
                },
                V::OpenFresh { epoch },
            ) if epoch.number() > old.number() => Ok((
                S::Open {
                    epoch,
                    next_sequence,
                },
                Vec::new(),
            )),
            (S::Open { .. } | S::Ended { .. }, V::OpenFresh { .. }) => Err(TransitionError::Stale),
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "queue_line_machine_test.rs"]
mod tests;
