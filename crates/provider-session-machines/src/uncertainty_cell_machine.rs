//! One binding's unknown outcomes, as a pure machine: the single place an
//! admission asks "is an earlier send's outcome unknown?" (D1 state machines;
//! design note §14.1). Every write to the set and every admission check run
//! under this one cell, so each admission sees one consistent moment. The
//! machine is generic over its operation and target keys; the Host uses its
//! protocol identifiers and words the refusal.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::marker::PhantomData;

/// What a key of the machine must be: an operation or a session target.
pub trait UncertaintyKey: Clone + Debug + Ord {}

impl<Key: Clone + Debug + Ord> UncertaintyKey for Key {}

/// Operations whose outcome is unknown and that no live owner remains for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownOutcomes<Operation, Target> {
    by_target: BTreeMap<Target, BTreeSet<Operation>>,
    /// Unknown operations with no target: they block every session on the
    /// binding, as today's scan treats them.
    binding_wide: BTreeSet<Operation>,
}

impl<Operation, Target> Default for UnknownOutcomes<Operation, Target> {
    fn default() -> Self {
        Self {
            by_target: BTreeMap::new(),
            binding_wide: BTreeSet::new(),
        }
    }
}

impl<Operation: UncertaintyKey, Target: UncertaintyKey> UnknownOutcomes<Operation, Target> {
    /// What blocks an admission on `target`; a create has no target yet, and
    /// only binding-wide unknowns block it, as today's scan does.
    fn blocking(&self, target: Option<&Target>) -> Vec<Operation> {
        self.binding_wide
            .iter()
            .chain(
                target
                    .and_then(|target| self.by_target.get(target))
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.binding_wide.is_empty() && self.by_target.values().all(BTreeSet::is_empty)
    }

    fn contains(&self, operation: &Operation) -> bool {
        self.binding_wide.contains(operation)
            || self.by_target.values().any(|set| set.contains(operation))
    }

    fn with(&self, operation: &Operation, target: Option<&Target>) -> Self {
        let mut next = self.clone();
        match target {
            Some(target) => {
                next.by_target
                    .entry(target.clone())
                    .or_default()
                    .insert(operation.clone());
            }
            None => {
                next.binding_wide.insert(operation.clone());
            }
        }
        next
    }

    fn without(&self, operation: &Operation) -> Self {
        let mut next = self.clone();
        next.binding_wide.remove(operation);
        for set in next.by_target.values_mut() {
            set.remove(operation);
        }
        next.by_target.retain(|_, set| !set.is_empty());
        next
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UncertaintyEvent<Operation, Target> {
    /// The operation's live owner records its unknown outcome, before its own
    /// cell becomes non-live.
    BecameUnknown {
        operation: Operation,
        target: Option<Target>,
    },
    /// Reconciliation committed durably.
    Reconciled { operation: Operation },
    /// An admission on `target` asks whether it may proceed.
    AdmissionCheck { target: Option<Target> },
}

/// The admission check's answer when it passes; refusals carry the blockers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UncertaintyEffect {
    Admit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UncertaintyRefusal<Operation> {
    /// An earlier send's outcome is unknown: these must be reconciled
    /// first. The Host words it (M8).
    EarlierOutcomeUnknown { operations: Vec<Operation> },
}

impl<Operation: Debug> RefusalTag for UncertaintyRefusal<Operation> {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::EarlierOutcomeUnknown { .. } => "earlier_outcome_unknown",
        }
    }
}

impl<Operation: UncertaintyKey, Target: UncertaintyKey> StateTag
    for UnknownOutcomes<Operation, Target>
{
    fn state_tag(&self) -> &'static str {
        if self.is_empty() { "clear" } else { "holding" }
    }

    fn is_terminal(&self) -> bool {
        false
    }
}

impl<Operation: Debug, Target: Debug> EventTag for UncertaintyEvent<Operation, Target> {
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

/// The machine for one binding, keyed by `Operation` and `Target`.
pub struct UncertaintyCellMachine<Operation, Target>(PhantomData<(Operation, Target)>);

impl<Operation: UncertaintyKey, Target: UncertaintyKey> StateMachine
    for UncertaintyCellMachine<Operation, Target>
{
    type State = UnknownOutcomes<Operation, Target>;
    type Event = UncertaintyEvent<Operation, Target>;
    type Effect = UncertaintyEffect;
    type Error = UncertaintyRefusal<Operation>;

    fn transition(
        state: &UnknownOutcomes<Operation, Target>,
        event: UncertaintyEvent<Operation, Target>,
    ) -> StateMachineResult<Self> {
        match event {
            UncertaintyEvent::BecameUnknown { operation, target } => {
                Ok((state.with(&operation, target.as_ref()), Vec::new()))
            }
            UncertaintyEvent::Reconciled { operation } if state.contains(&operation) => {
                Ok((state.without(&operation), Vec::new()))
            }
            UncertaintyEvent::Reconciled { .. } => Err(TransitionError::Stale),
            UncertaintyEvent::AdmissionCheck { target } => {
                let blocking = state.blocking(target.as_ref());
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
pub fn seeded<'a, Operation: UncertaintyKey + 'a, Target: UncertaintyKey + 'a>(
    rows: impl IntoIterator<Item = (&'a Operation, Option<&'a Target>)>,
) -> UnknownOutcomes<Operation, Target> {
    rows.into_iter()
        .fold(UnknownOutcomes::default(), |seed, (operation, target)| {
            seed.with(operation, target)
        })
}

#[cfg(test)]
#[path = "uncertainty_cell_machine_test.rs"]
mod tests;
