use crate::engine_context::{Context, EngineTime};
use serde::{Serialize, de::DeserializeOwned};
use state_machine_core::{EventTag, RefusalTag, StateTag, Transition, TransitionError};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MachineKind(pub &'static str);

/// The raw stored key is retained even when its contents are corrupt.
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InstanceKey(pub String);

pub type TransitionResult<M> = Result<
    (<M as Machine>::State, Vec<<M as Machine>::Effect>),
    TransitionError<<M as Machine>::Error>,
>;

/// A reducer receives time and identifiers; only its effect handler performs I/O.
///
/// This complete machine compiles. Its sibling differs only by the missing arm.
/// ```
/// # #![deny(clippy::wildcard_enum_match_arm)]
/// use central_engine_model::*;
/// #[derive(Clone)] enum State { Waiting, Finished }
/// impl StateTag for State {
///     fn state_tag(&self) -> &'static str { match self { Self::Waiting => "waiting", Self::Finished => "finished" } }
///     fn is_terminal(&self) -> bool { match self { Self::Waiting => false, Self::Finished => true } }
/// }
/// #[derive(Clone)] enum Event { Due }
/// impl EventTag for Event { fn event_tag(&self) -> &'static str { "due" } }
/// #[derive(serde::Serialize, serde::Deserialize)] enum Refusal { Finished }
/// impl RefusalTag for Refusal { fn refusal_tag(&self) -> &'static str { "finished" } }
/// struct Example;
/// impl Machine for Example {
///     const KIND: MachineKind = MachineKind("example");
///     type State = State; type Event = Event; type Effect = (); type Error = Refusal;
///     fn transition(state: &State, event: Event, _: &mut Context<'_>) -> Result<(State, Vec<()>), TransitionError<Refusal>> {
///         match (state, event) {
///             (State::Waiting, Event::Due) => Ok((State::Finished, vec![])),
///             (State::Finished, Event::Due) => Err(TransitionError::Stale),
///         }
///     }
///     fn transitions() -> &'static [Transition] { &[Transition { from: "waiting", event: "due", to: "finished", terminal: true }] }
///     fn next_due(_: &State) -> Option<EngineTime> { None }
/// }
/// ```
///
/// Omitting the finished-state arm is non-exhaustive (E0004).
/// ```compile_fail
/// # #![deny(clippy::wildcard_enum_match_arm)]
/// use central_engine_model::*;
/// #[derive(Clone)] enum State { Waiting, Finished }
/// impl StateTag for State {
///     fn state_tag(&self) -> &'static str { match self { Self::Waiting => "waiting", Self::Finished => "finished" } }
///     fn is_terminal(&self) -> bool { match self { Self::Waiting => false, Self::Finished => true } }
/// }
/// #[derive(Clone)] enum Event { Due }
/// impl EventTag for Event { fn event_tag(&self) -> &'static str { "due" } }
/// #[derive(serde::Serialize, serde::Deserialize)] enum Refusal { Finished }
/// impl RefusalTag for Refusal { fn refusal_tag(&self) -> &'static str { "finished" } }
/// struct Example;
/// impl Machine for Example {
///     const KIND: MachineKind = MachineKind("example");
///     type State = State; type Event = Event; type Effect = (); type Error = Refusal;
///     fn transition(state: &State, event: Event, _: &mut Context<'_>) -> Result<(State, Vec<()>), TransitionError<Refusal>> {
///         match (state, event) {
///             (State::Waiting, Event::Due) => Ok((State::Finished, vec![])),
///         }
///     }
///     fn transitions() -> &'static [Transition] { &[Transition { from: "waiting", event: "due", to: "finished", terminal: true }] }
///     fn next_due(_: &State) -> Option<EngineTime> { None }
/// }
/// ```
pub trait Machine {
    const KIND: MachineKind;
    type State: Clone + StateTag;
    type Event: Clone + EventTag;
    type Effect;
    type Error: RefusalTag + Serialize + DeserializeOwned;

    fn transition(
        state: &Self::State,
        event: Self::Event,
        context: &mut Context<'_>,
    ) -> TransitionResult<Self>;

    fn transitions() -> &'static [Transition];
    fn next_due(state: &Self::State) -> Option<EngineTime>;
    fn retirement_threshold() -> crate::RetirementThreshold {
        crate::RetirementThreshold::DEFAULT
    }
}
