use crate::{EventTag, RefusalTag, StateTag, Transition, TransitionError};

pub type StateMachineResult<M> = Result<
    (<M as StateMachine>::State, Vec<<M as StateMachine>::Effect>),
    TransitionError<<M as StateMachine>::Error>,
>;

/// One item's machine: its states, events, effects and the table of declared
/// moves. `transition` is pure: it reads only the current state and the event
/// (anything else it needs, such as a time, travels in the event) and returns
/// the next state with the effects its caller runs afterwards. A move not in
/// the table is refused as `NotAllowed`; an event from an older run of the
/// item is `Stale`.
///
/// Machines deny `clippy::wildcard_enum_match_arm` and match `(state, event)`
/// exhaustively, so a new state or event doesn't compile until every row is
/// decided. [`check_state_machine_transitions`](crate::check_state_machine_transitions)
/// then proves the reducer takes exactly the declared table.
///
/// This complete machine compiles. Its sibling differs only by the missing arm.
/// ```
/// # #![deny(clippy::wildcard_enum_match_arm)]
/// use state_machine_core::*;
/// #[derive(Clone)] enum State { Waiting, Finished }
/// impl StateTag for State {
///     fn state_tag(&self) -> &'static str { match self { Self::Waiting => "waiting", Self::Finished => "finished" } }
///     fn is_terminal(&self) -> bool { match self { Self::Waiting => false, Self::Finished => true } }
/// }
/// #[derive(Clone)] enum Event { Due }
/// impl EventTag for Event { fn event_tag(&self) -> &'static str { "due" } }
/// enum Refusal { Finished }
/// impl RefusalTag for Refusal { fn refusal_tag(&self) -> &'static str { "finished" } }
/// struct Example;
/// impl StateMachine for Example {
///     type State = State; type Event = Event; type Effect = (); type Error = Refusal;
///     fn transition(state: &State, event: Event) -> Result<(State, Vec<()>), TransitionError<Refusal>> {
///         match (state, event) {
///             (State::Waiting, Event::Due) => Ok((State::Finished, vec![])),
///             (State::Finished, Event::Due) => Err(TransitionError::Stale),
///         }
///     }
///     fn transitions() -> &'static [Transition] { &[Transition { from: "waiting", event: "due", to: "finished", terminal: true }] }
/// }
/// ```
///
/// Omitting the finished-state arm is non-exhaustive (E0004).
/// ```compile_fail
/// # #![deny(clippy::wildcard_enum_match_arm)]
/// use state_machine_core::*;
/// #[derive(Clone)] enum State { Waiting, Finished }
/// impl StateTag for State {
///     fn state_tag(&self) -> &'static str { match self { Self::Waiting => "waiting", Self::Finished => "finished" } }
///     fn is_terminal(&self) -> bool { match self { Self::Waiting => false, Self::Finished => true } }
/// }
/// #[derive(Clone)] enum Event { Due }
/// impl EventTag for Event { fn event_tag(&self) -> &'static str { "due" } }
/// enum Refusal { Finished }
/// impl RefusalTag for Refusal { fn refusal_tag(&self) -> &'static str { "finished" } }
/// struct Example;
/// impl StateMachine for Example {
///     type State = State; type Event = Event; type Effect = (); type Error = Refusal;
///     fn transition(state: &State, event: Event) -> Result<(State, Vec<()>), TransitionError<Refusal>> {
///         match (state, event) {
///             (State::Waiting, Event::Due) => Ok((State::Finished, vec![])),
///         }
///     }
///     fn transitions() -> &'static [Transition] { &[Transition { from: "waiting", event: "due", to: "finished", terminal: true }] }
/// }
/// ```
pub trait StateMachine {
    type State: Clone + StateTag;
    type Event: Clone + EventTag;
    type Effect;
    type Error: RefusalTag;

    fn transition(state: &Self::State, event: Self::Event) -> StateMachineResult<Self>;

    fn transitions() -> &'static [Transition];
}
