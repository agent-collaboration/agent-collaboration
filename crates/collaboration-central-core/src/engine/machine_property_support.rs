use central_engine_model::{
    Context, EngineTime, EventTag, IdSource, Machine, StateTag, Transition, TransitionResult,
};
use std::fmt::Debug;

struct ReplayIds(u64);
impl IdSource for ReplayIds {
    fn next_id(&mut self) -> uuid::Uuid {
        self.0 += 1;
        format!("01890f2e-7b4c-7cc0-98c4-{:012x}", self.0)
            .parse()
            .unwrap()
    }
}

/// Replay exactly the same supplied time and identifier stream before accepting
/// a step. Only a returned success is installed by the sequence's caller.
pub(crate) fn checked_step<M>(
    state: &M::State,
    event: M::Event,
    now: EngineTime,
    step: u64,
) -> TransitionResult<M>
where
    M: Machine,
    M::State: Debug + Eq,
    M::Effect: Debug + Eq,
    M::Error: Debug + Eq,
{
    let before = state.clone();
    let event_tag = event.event_tag();
    let reduce = || {
        M::transition(
            state,
            event.clone(),
            &mut Context {
                now,
                ids: &mut ReplayIds(step),
            },
        )
    };
    let first = reduce();
    let replay = reduce();
    assert_eq!(
        first, replay,
        "same inputs, time and identifiers must give the same result"
    );
    assert_eq!(state, &before, "the reducer must not mutate its input");
    if let Ok((next, _)) = &first {
        let edge = Transition {
            from: state.state_tag(),
            event: event_tag,
            to: next.state_tag(),
            terminal: next.is_terminal(),
        };
        assert!(
            M::transitions().contains(&edge),
            "undeclared edge: {edge:?}"
        );
        if state.is_terminal() {
            assert!(next.is_terminal(), "finished work cannot restart");
        }
    }
    first
}
