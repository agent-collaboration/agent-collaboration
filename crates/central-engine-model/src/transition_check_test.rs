#![deny(clippy::wildcard_enum_match_arm)]
use crate::*;

#[derive(Clone, Debug, Eq, PartialEq)]
enum State {
    Waiting,
    Fired,
    Stopped,
}
impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Fired => "fired",
            Self::Stopped => "stopped",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Waiting => false,
            Self::Fired | Self::Stopped => true,
        }
    }
}
#[derive(Clone)]
enum Event {
    Due,
    Stop,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Stop => "stop",
        }
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum Refusal {
    Finished,
}
impl RefusalTag for Refusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::Finished => "finished",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Effect {
    message_id: uuid::Uuid,
    fired_at: EngineTime,
}
struct Probe;
impl Machine for Probe {
    const KIND: MachineKind = MachineKind("transition_test");
    type State = State;
    type Event = Event;
    type Effect = Effect;
    type Error = Refusal;
    fn transition(
        state: &State,
        event: Event,
        context: &mut Context<'_>,
    ) -> Result<(State, Vec<Effect>), TransitionError<Refusal>> {
        match (state, event) {
            (State::Waiting, Event::Due) => Ok((
                State::Fired,
                vec![Effect {
                    message_id: context.ids.next_id(),
                    fired_at: context.now,
                }],
            )),
            (State::Waiting, Event::Stop) => Ok((State::Stopped, vec![])),
            (State::Fired | State::Stopped, Event::Due) => Err(TransitionError::Stale),
            (State::Fired | State::Stopped, Event::Stop) => {
                Err(TransitionError::Domain(Refusal::Finished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "waiting",
                event: "due",
                to: "fired",
                terminal: true,
            },
            Transition {
                from: "waiting",
                event: "stop",
                to: "stopped",
                terminal: true,
            },
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Waiting => EngineTime::from_timestamp_millis(300_000),
            State::Fired | State::Stopped => None,
        }
    }
}
struct ReplayIds;
impl IdSource for ReplayIds {
    fn next_id(&mut self) -> uuid::Uuid {
        "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap()
    }
}
fn context(ids: &mut ReplayIds) -> Context<'_> {
    Context {
        now: EngineTime::from_timestamp_millis(300_000).unwrap(),
        ids,
    }
}

#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    let cases = [State::Waiting, State::Fired, State::Stopped]
        .into_iter()
        .flat_map(|state| {
            [Event::Due, Event::Stop]
                .into_iter()
                .map(move |event| (state.clone(), event))
        });
    assert_eq!(
        check_transitions::<Probe>(cases, &mut context(&mut ReplayIds)),
        Ok(())
    );
}

#[test]
fn a_declared_edge_not_reached_is_reported_by_name() {
    assert_eq!(
        check_transitions::<Probe>([(State::Waiting, Event::Due)], &mut context(&mut ReplayIds)),
        Err(TransitionCheckError::Unreachable(Probe::transitions()[1]))
    );
}

struct UndeclaredProbe;
impl Machine for UndeclaredProbe {
    const KIND: MachineKind = Probe::KIND;
    type State = State;
    type Event = Event;
    type Effect = Effect;
    type Error = Refusal;
    fn transition(
        state: &State,
        event: Event,
        context: &mut Context<'_>,
    ) -> Result<(State, Vec<Effect>), TransitionError<Refusal>> {
        Probe::transition(state, event, context)
    }
    fn transitions() -> &'static [Transition] {
        &[]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        Probe::next_due(state)
    }
}

#[test]
fn an_undeclared_edge_is_reported_by_name() {
    assert_eq!(
        check_transitions::<UndeclaredProbe>(
            [(State::Waiting, Event::Due)],
            &mut context(&mut ReplayIds)
        ),
        Err(TransitionCheckError::Undeclared(Probe::transitions()[0]))
    );
}

#[test]
fn equal_inputs_with_replayed_identifiers_give_equal_effects() {
    let first =
        Probe::transition(&State::Waiting, Event::Due, &mut context(&mut ReplayIds)).unwrap();
    let second =
        Probe::transition(&State::Waiting, Event::Due, &mut context(&mut ReplayIds)).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.1[0].message_id.get_version_num(), 7);
    assert!(matches!(
        Probe::transition(&State::Fired, Event::Due, &mut context(&mut ReplayIds)),
        Err(TransitionError::Stale)
    ));
}

#[test]
fn due_batch_policy_is_the_owner_selected_thirty_fifty_two_hundred_fifty() {
    assert_eq!(
        DueBatchPolicy::DEFAULT.spacing,
        chrono::Duration::seconds(30)
    );
    assert_eq!(
        DueBatchPolicy::DEFAULT.look_ahead,
        chrono::Duration::seconds(50)
    );
    assert_eq!(DueBatchPolicy::DEFAULT.per_kind_cap, 250);
}
