use crate::{Context, EventTag, Machine, StateTag, Transition};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TransitionCheckError {
    #[error("machine produced an undeclared transition: {0:?}")]
    Undeclared(Transition),
    #[error("declared transition was not reached: {0:?}")]
    Unreachable(Transition),
}

/// Supply all state/event fixtures, including payload cases that select distinct edges.
pub fn check_transitions<M: Machine>(
    cases: impl IntoIterator<Item = (M::State, M::Event)>,
    context: &mut Context<'_>,
) -> Result<(), TransitionCheckError> {
    let mut reached = Vec::new();
    for (state, event) in cases {
        let from = state.state_tag();
        let event_tag = event.event_tag();
        if let Ok((next, _)) = M::transition(&state, event, context) {
            let transition = Transition {
                from,
                event: event_tag,
                to: next.state_tag(),
                terminal: next.is_terminal(),
            };
            if !M::transitions().contains(&transition) {
                return Err(TransitionCheckError::Undeclared(transition));
            }
            if !reached.contains(&transition) {
                reached.push(transition);
            }
        }
    }
    for transition in M::transitions() {
        if !reached.contains(transition) {
            return Err(TransitionCheckError::Unreachable(*transition));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "transition_check_test.rs"]
mod tests;
