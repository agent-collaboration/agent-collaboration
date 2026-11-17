//! One provider operation's record, as a pure machine (D1 state machines;
//! design note §11.5, §12.5, §14.1, §15.3, §16). Its events are durable
//! outcomes: the runner applies one only after its conditional write
//! committed, so a failed write is no transition. A dispatch attempt carries
//! an in-memory UUIDv7, so a late completion from an earlier attempt is Stale.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

/// One dispatch attempt's identity (a UUIDv7, held in memory only).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct DispatchAttempt(pub(crate) uuid::Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettledEffect {
    None,
    Applied,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reconciliation {
    Unresolved,
    Confirmed,
    NotReconcilable,
}

/// How a dispatched attempt ended, as the completion task classified it
/// through today's error mapping (design note §16).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptEnding {
    Applied,
    /// The agent's own definite refusal (−32000, −32002, −32601, −32602).
    Refused,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRecord {
    Reserving,
    /// The reservation ended without a row of its own (a durable conflict or
    /// a failed insert); the index entry goes.
    Dropped,
    Admitted {
        queued: bool,
    },
    MayHaveDispatched {
        attempt: DispatchAttempt,
    },
    /// Unknown, still live: waiting for the binding's uncertainty cell to
    /// record it before the operation stops counting as live (§14.1).
    SettlingUnknown {
        attempt: DispatchAttempt,
    },
    Settled {
        effect: SettledEffect,
        reconciliation: Reconciliation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRecordEvent {
    DurableInserted,
    DurableConflict,
    DurableInsertFailed,
    Queued,
    DispatchMarked {
        attempt: DispatchAttempt,
    },
    Undispatched {
        attempt: DispatchAttempt,
    },
    NotSent,
    Completed {
        attempt: DispatchAttempt,
        ending: AttemptEnding,
    },
    UncertaintyRecorded {
        attempt: DispatchAttempt,
    },
    Reconciled {
        reconciliation: Reconciliation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRecordEffect {
    /// Answer the caller: admitted, or answer from the existing row.
    AnswerAdmitted,
    AnswerFromExistingRow,
    AnswerNotRecorded,
    /// Tell the binding's uncertainty cell before going non-live.
    RecordUncertainty,
    /// Write the terminal row (the runner owns the write and its overlay).
    WriteTerminal(SettledEffect),
    /// Reconciliation committed: clear it from the uncertainty cell.
    ClearUncertainty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationRecordRefusal {
    Settled,
}

impl RefusalTag for OperationRecordRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::Settled => "settled",
        }
    }
}

impl StateTag for OperationRecord {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Reserving => "reserving",
            Self::Dropped => "dropped",
            Self::Admitted { queued: false } => "admitted",
            Self::Admitted { queued: true } => "admitted_queued",
            Self::MayHaveDispatched { .. } => "may_have_dispatched",
            Self::SettlingUnknown { .. } => "settling_unknown",
            Self::Settled {
                reconciliation: Reconciliation::Unresolved,
                ..
            } => "settled_unresolved",
            Self::Settled { .. } => "settled",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Dropped
            | Self::Settled {
                reconciliation: Reconciliation::Confirmed | Reconciliation::NotReconcilable,
                ..
            } => true,
            Self::Reserving
            | Self::Admitted { .. }
            | Self::MayHaveDispatched { .. }
            | Self::SettlingUnknown { .. }
            | Self::Settled {
                reconciliation: Reconciliation::Unresolved,
                ..
            } => false,
        }
    }
}

impl EventTag for OperationRecordEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::DurableInserted => "durable_inserted",
            Self::DurableConflict => "durable_conflict",
            Self::DurableInsertFailed => "durable_insert_failed",
            Self::Queued => "queued",
            Self::DispatchMarked { .. } => "dispatch_marked",
            Self::Undispatched { .. } => "undispatched",
            Self::NotSent => "not_sent",
            Self::Completed { .. } => "completed",
            Self::UncertaintyRecorded { .. } => "uncertainty_recorded",
            Self::Reconciled { .. } => "reconciled",
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
    edge("reserving", "durable_inserted", "admitted", false),
    edge("reserving", "durable_conflict", "dropped", true),
    edge("reserving", "durable_insert_failed", "dropped", true),
    edge("admitted", "queued", "admitted_queued", false),
    edge("admitted", "dispatch_marked", "may_have_dispatched", false),
    edge(
        "admitted_queued",
        "dispatch_marked",
        "may_have_dispatched",
        false,
    ),
    edge("may_have_dispatched", "undispatched", "admitted", false),
    edge("admitted", "not_sent", "settled", true),
    edge("admitted_queued", "not_sent", "settled", true),
    edge("may_have_dispatched", "completed", "settled", true),
    edge(
        "may_have_dispatched",
        "completed",
        "settling_unknown",
        false,
    ),
    edge(
        "settling_unknown",
        "uncertainty_recorded",
        "settled_unresolved",
        false,
    ),
    edge("settled_unresolved", "reconciled", "settled", true),
];

pub(crate) struct OperationRecordMachine;

impl StateMachine for OperationRecordMachine {
    type State = OperationRecord;
    type Event = OperationRecordEvent;
    type Effect = OperationRecordEffect;
    type Error = OperationRecordRefusal;

    fn transition(
        state: &OperationRecord,
        event: OperationRecordEvent,
    ) -> StateMachineResult<Self> {
        use OperationRecord as S;
        use OperationRecordEffect as E;
        use OperationRecordEvent as V;
        let settled = |effect| S::Settled {
            effect,
            reconciliation: Reconciliation::Confirmed,
        };
        match (*state, event) {
            (S::Reserving, V::DurableInserted) => {
                Ok((S::Admitted { queued: false }, vec![E::AnswerAdmitted]))
            }
            (S::Reserving, V::DurableConflict) => Ok((S::Dropped, vec![E::AnswerFromExistingRow])),
            (S::Reserving, V::DurableInsertFailed) => Ok((S::Dropped, vec![E::AnswerNotRecorded])),
            (S::Admitted { queued: false }, V::Queued) => {
                Ok((S::Admitted { queued: true }, Vec::new()))
            }
            (S::Admitted { .. }, V::DispatchMarked { attempt }) => {
                Ok((S::MayHaveDispatched { attempt }, Vec::new()))
            }
            (S::MayHaveDispatched { attempt: current }, V::Undispatched { attempt })
                if current == attempt =>
            {
                Ok((S::Admitted { queued: false }, Vec::new()))
            }
            (S::Admitted { .. }, V::NotSent) => Ok((
                settled(SettledEffect::None),
                vec![E::WriteTerminal(SettledEffect::None)],
            )),
            (
                S::MayHaveDispatched { attempt: current },
                V::Completed {
                    attempt,
                    ending: AttemptEnding::Applied,
                },
            ) if current == attempt => Ok((
                settled(SettledEffect::Applied),
                vec![E::WriteTerminal(SettledEffect::Applied)],
            )),
            (
                S::MayHaveDispatched { attempt: current },
                V::Completed {
                    attempt,
                    ending: AttemptEnding::Refused,
                },
            ) if current == attempt => Ok((
                settled(SettledEffect::None),
                vec![E::WriteTerminal(SettledEffect::None)],
            )),
            (
                S::MayHaveDispatched { attempt: current },
                V::Completed {
                    attempt,
                    ending: AttemptEnding::Unknown,
                },
            ) if current == attempt => {
                Ok((S::SettlingUnknown { attempt }, vec![E::RecordUncertainty]))
            }
            (S::SettlingUnknown { attempt: current }, V::UncertaintyRecorded { attempt })
                if current == attempt =>
            {
                Ok((
                    S::Settled {
                        effect: SettledEffect::Unknown,
                        reconciliation: Reconciliation::Unresolved,
                    },
                    vec![E::WriteTerminal(SettledEffect::Unknown)],
                ))
            }
            (
                S::Settled {
                    effect,
                    reconciliation: Reconciliation::Unresolved,
                },
                V::Reconciled { reconciliation },
            ) if reconciliation != Reconciliation::Unresolved => Ok((
                S::Settled {
                    effect,
                    reconciliation,
                },
                vec![E::ClearUncertainty],
            )),
            (
                S::Settled { .. } | S::Dropped,
                V::DurableInserted
                | V::DurableConflict
                | V::DurableInsertFailed
                | V::Queued
                | V::DispatchMarked { .. }
                | V::Undispatched { .. }
                | V::NotSent
                | V::Completed { .. }
                | V::UncertaintyRecorded { .. }
                | V::Reconciled { .. },
            ) => Err(TransitionError::Domain(OperationRecordRefusal::Settled)),
            (
                S::Reserving
                | S::Admitted { .. }
                | S::MayHaveDispatched { .. }
                | S::SettlingUnknown { .. },
                V::Undispatched { .. } | V::Completed { .. } | V::UncertaintyRecorded { .. },
            ) => Err(TransitionError::Stale),
            (
                S::Reserving
                | S::Admitted { .. }
                | S::MayHaveDispatched { .. }
                | S::SettlingUnknown { .. },
                V::DurableInserted
                | V::DurableConflict
                | V::DurableInsertFailed
                | V::Queued
                | V::DispatchMarked { .. }
                | V::NotSent
                | V::Reconciled { .. },
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
#[path = "operation_record_machine_test.rs"]
mod tests;
