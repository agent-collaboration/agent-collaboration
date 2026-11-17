use crate::{EventTag, StateMachine, StateTag, Transition};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TransitionCheckError {
    #[error("machine produced an undeclared transition: {0:?}")]
    Undeclared(Transition),
    #[error("declared transition was not reached: {0:?}")]
    Unreachable(Transition),
}

/// Checks a reducer against its declared table: every transition `apply`
/// takes for the supplied cases is declared, and every declared transition is
/// taken by some case. `apply` returns the next state, or `None` when the
/// reducer refused the event. Cases run in the order given, and `apply` is
/// called once per case.
///
/// Supply all state/event fixtures, including payload cases that select
/// distinct edges.
pub fn check_declared_transitions<State, Event, Next>(
    cases: impl IntoIterator<Item = (State, Event)>,
    declared: &[Transition],
    mut apply: impl FnMut(&State, Event) -> Option<Next>,
) -> Result<(), TransitionCheckError>
where
    State: StateTag,
    Event: EventTag,
    Next: StateTag,
{
    let mut reached = Vec::new();
    for (state, event) in cases {
        let from = state.state_tag();
        let event_tag = event.event_tag();
        if let Some(next) = apply(&state, event) {
            let transition = Transition {
                from,
                event: event_tag,
                to: next.state_tag(),
                terminal: next.is_terminal(),
            };
            if !declared.contains(&transition) {
                return Err(TransitionCheckError::Undeclared(transition));
            }
            if !reached.contains(&transition) {
                reached.push(transition);
            }
        }
    }
    for transition in declared {
        if !reached.contains(transition) {
            return Err(TransitionCheckError::Unreachable(*transition));
        }
    }
    Ok(())
}

/// [`check_declared_transitions`] for a [`StateMachine`].
pub fn check_state_machine_transitions<M: StateMachine>(
    cases: impl IntoIterator<Item = (M::State, M::Event)>,
) -> Result<(), TransitionCheckError> {
    check_declared_transitions(cases, M::transitions(), |state, event| {
        M::transition(state, event).ok().map(|(next, _)| next)
    })
}

#[cfg(test)]
#[path = "transition_check_test.rs"]
mod tests;
