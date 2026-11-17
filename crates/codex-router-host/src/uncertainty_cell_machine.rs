//! One binding's unknown outcomes, as a pure machine: the single place an
//! admission asks "is an earlier send's outcome unknown?" (D1 state machines;
//! design note §14.1). Every write to the set and every admission check run
//! under this one cell, so each admission sees one consistent moment.

use collaboration_protocol::{OperationId, SessionRef};
use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};
use std::collections::{BTreeMap, BTreeSet};

/// Operations whose outcome is unknown and that no live owner remains for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct UnknownOutcomes {
    by_target: BTreeMap<String, BTreeSet<String>>,
    /// Unknown operations with no target: they block every session on the
    /// binding, as today's scan treats them.
    binding_wide: BTreeSet<String>,
}

fn target_key(target: &SessionRef) -> String {
    format!(
        "{}/{}/{}",
        String::from(target.endpoint.service_id.clone()),
        String::from(target.endpoint.endpoint_id.clone()),
        String::from(target.session_id.clone())
    )
}

impl UnknownOutcomes {
    fn blocking(&self, target: &SessionRef) -> Vec<String> {
        self.binding_wide
            .iter()
            .chain(
                self.by_target
                    .get(&target_key(target))
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.binding_wide.is_empty() && self.by_target.values().all(BTreeSet::is_empty)
    }

    fn contains(&self, operation: &str) -> bool {
        self.binding_wide.contains(operation)
            || self.by_target.values().any(|set| set.contains(operation))
    }

    fn with(&self, operation: &OperationId, target: Option<&SessionRef>) -> Self {
        let mut next = self.clone();
        let id = operation.as_str().to_owned();
        match target {
            Some(target) => {
                next.by_target
                    .entry(target_key(target))
                    .or_default()
                    .insert(id);
            }
            None => {
                next.binding_wide.insert(id);
            }
        }
        next
    }

    fn without(&self, operation: &OperationId) -> Self {
        let mut next = self.clone();
        next.binding_wide.remove(operation.as_str());
        for set in next.by_target.values_mut() {
            set.remove(operation.as_str());
        }
        next.by_target.retain(|_, set| !set.is_empty());
        next
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UncertaintyEvent {
    /// The operation's live owner records its unknown outcome, before its own
    /// cell becomes non-live.
    BecameUnknown {
        operation: OperationId,
        target: Option<SessionRef>,
    },
    /// Reconciliation committed durably.
    Reconciled { operation: OperationId },
    /// An admission on `target` asks whether it may proceed.
    AdmissionCheck { target: SessionRef },
}

/// The admission check's answer when it passes; refusals carry the blockers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UncertaintyEffect {
    Admit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UncertaintyRefusal {
    /// "an earlier send's outcome is unknown; reconcile <ids> first" (M8).
    EarlierOutcomeUnknown { operations: Vec<String> },
}

impl RefusalTag for UncertaintyRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::EarlierOutcomeUnknown { .. } => "earlier_outcome_unknown",
        }
    }
}

impl UncertaintyRefusal {
    pub(crate) fn human_answer(&self) -> String {
        match self {
            Self::EarlierOutcomeUnknown { operations } => format!(
                "an earlier send's outcome is unknown; reconcile {} first",
                operations.join(", ")
            ),
        }
    }
}

impl StateTag for UnknownOutcomes {
    fn state_tag(&self) -> &'static str {
        if self.is_empty() { "clear" } else { "holding" }
    }

    fn is_terminal(&self) -> bool {
        false
    }
}

impl EventTag for UncertaintyEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::BecameUnknown { .. } => "became_unknown",
            Self::Reconciled { .. } => "reconciled",
            Self::AdmissionCheck { .. } => "admission_check",
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
    edge("clear", "became_unknown", "holding"),
    edge("holding", "became_unknown", "holding"),
    edge("holding", "reconciled", "holding"),
    edge("holding", "reconciled", "clear"),
    edge("clear", "admission_check", "clear"),
    edge("holding", "admission_check", "holding"),
];

pub(crate) struct UncertaintyCellMachine;

impl StateMachine for UncertaintyCellMachine {
    type State = UnknownOutcomes;
    type Event = UncertaintyEvent;
    type Effect = UncertaintyEffect;
    type Error = UncertaintyRefusal;

    fn transition(state: &UnknownOutcomes, event: UncertaintyEvent) -> StateMachineResult<Self> {
        match event {
            UncertaintyEvent::BecameUnknown { operation, target } => {
                Ok((state.with(&operation, target.as_ref()), Vec::new()))
            }
            UncertaintyEvent::Reconciled { operation } if state.contains(operation.as_str()) => {
                Ok((state.without(&operation), Vec::new()))
            }
            UncertaintyEvent::Reconciled { .. } => Err(TransitionError::Stale),
            UncertaintyEvent::AdmissionCheck { target } => {
                let blocking = state.blocking(&target);
                if blocking.is_empty() {
                    Ok((state.clone(), vec![UncertaintyEffect::Admit]))
                } else {
                    Err(TransitionError::Domain(
                        UncertaintyRefusal::EarlierOutcomeUnknown {
                            operations: blocking,
                        },
                    ))
                }
            }
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

/// The start-up seed: today's durable scan of unknown rows, before any
/// admission on the binding (design note §14.1).
pub(crate) fn seeded<'a>(
    rows: impl IntoIterator<Item = (&'a OperationId, Option<&'a SessionRef>)>,
) -> UnknownOutcomes {
    rows.into_iter()
        .fold(UnknownOutcomes::default(), |seed, (operation, target)| {
            seed.with(operation, target)
        })
}

#[cfg(test)]
#[path = "uncertainty_cell_machine_test.rs"]
mod tests;
