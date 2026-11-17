use crate::{Context, Machine};
use state_machine_core::{TransitionCheckError, check_declared_transitions};

/// Supply all state/event fixtures, including payload cases that select distinct edges.
pub fn check_transitions<M: Machine>(
    cases: impl IntoIterator<Item = (M::State, M::Event)>,
    context: &mut Context<'_>,
) -> Result<(), TransitionCheckError> {
    check_declared_transitions(cases, M::transitions(), |state, event| {
        M::transition(state, event, context)
            .ok()
            .map(|(next, _)| next)
    })
}

#[cfg(test)]
#[path = "transition_check_test.rs"]
mod tests;
