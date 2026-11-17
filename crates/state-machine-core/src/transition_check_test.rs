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

const DECLARED: &[Transition] = &[
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
];

struct Timer;
impl StateMachine for Timer {
    type State = State;
    type Event = Event;
    type Effect = &'static str;
    type Error = Refusal;
    fn transition(state: &State, event: Event) -> StateMachineResult<Self> {
        match (state, event) {
            (State::Waiting, Event::Due) => Ok((State::Fired, vec!["fire"])),
            (State::Waiting, Event::Stop) => Ok((State::Stopped, vec![])),
            (State::Fired | State::Stopped, Event::Due | Event::Stop) => {
                Err(TransitionError::Domain(Refusal::Finished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        DECLARED
    }
}

fn every_case() -> Vec<(State, Event)> {
    [State::Waiting, State::Fired, State::Stopped]
        .into_iter()
        .flat_map(|state| [(state.clone(), Event::Due), (state, Event::Stop)])
        .collect()
}

/// Oracle: the shared contract. Every state × event pair of a machine whose
/// reducer matches its table passes, and refused pairs count as no move.
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let cases = every_case();

    // Act
    let checked = check_state_machine_transitions::<Timer>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

/// Oracle: a declared edge no case reaches is reported by name.
#[test]
fn a_declared_edge_not_reached_is_reported_by_name() {
    // Arrange: no case stops a waiting timer.
    let cases = vec![(State::Waiting, Event::Due)];

    // Act
    let checked = check_state_machine_transitions::<Timer>(cases);

    // Assert
    assert_eq!(checked, Err(TransitionCheckError::Unreachable(DECLARED[1])));
}

/// Oracle: a move the reducer takes that the table doesn't declare is
/// reported by name, from the generic check every machine shares.
#[test]
fn an_undeclared_edge_is_reported_by_name() {
    // Arrange: a reducer that also fires a stopped timer.
    let cases = every_case();

    // Act
    let checked =
        check_declared_transitions(cases, DECLARED, |state, event| match (state, event) {
            (State::Stopped, Event::Due) => Some(State::Fired),
            (state, event) => Timer::transition(state, event).ok().map(|(next, _)| next),
        });

    // Assert
    assert_eq!(
        checked,
        Err(TransitionCheckError::Undeclared(Transition {
            from: "stopped",
            event: "due",
            to: "fired",
            terminal: true,
        }))
    );
}
