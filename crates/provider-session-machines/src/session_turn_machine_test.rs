#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::check_state_machine_transitions;

fn turn(id: u128) -> TurnId {
    TurnId(uuid::Uuid::from_u128(id))
}

/// Oracle: design note §3.5 and §12.4 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let states = [
        SessionTurn::Idle,
        SessionTurn::Running { turn: turn(1) },
        SessionTurn::Cancelling { turn: turn(1) },
    ];
    let events = [
        SessionTurnEvent::PromptStarted { turn: turn(2) },
        SessionTurnEvent::SteerRequested { agent_steers: true },
        SessionTurnEvent::SteerRequested {
            agent_steers: false,
        },
        SessionTurnEvent::CancelRequested,
        SessionTurnEvent::TurnEnded { turn: turn(1) },
        SessionTurnEvent::TurnEnded { turn: turn(9) },
    ];
    let cases = states
        .into_iter()
        .flat_map(|state| events.into_iter().map(move |event| (state, event)));

    // Act
    let checked = check_state_machine_transitions::<SessionTurnMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

/// Oracle: R12 and M8. A non-steering agent's busy session answers "cannot
/// steer: queue or interrupt"; an idle one has no turn to steer or cancel.
#[test]
fn steer_and_cancel_refusals_name_the_state_and_next_steps() {
    // Arrange
    let running = SessionTurn::Running { turn: turn(1) };

    // Act
    let busy_non_steering = SessionTurnMachine::transition(
        &running,
        SessionTurnEvent::SteerRequested {
            agent_steers: false,
        },
    );
    let idle_steer = SessionTurnMachine::transition(
        &SessionTurn::Idle,
        SessionTurnEvent::SteerRequested { agent_steers: true },
    );
    let busy_prompt =
        SessionTurnMachine::transition(&running, SessionTurnEvent::PromptStarted { turn: turn(2) });

    // Assert
    let phrase = |result: StateMachineResult<SessionTurnMachine>| match result {
        Err(TransitionError::Domain(refusal)) => refusal.human_phrase(),
        Ok(_) | Err(TransitionError::NotAllowed { .. } | TransitionError::Stale) => "",
    };
    assert_eq!(
        phrase(busy_non_steering),
        "cannot steer: queue or interrupt"
    );
    assert_eq!(
        phrase(idle_steer),
        "no running turn to steer; send it as a prompt"
    );
    assert_eq!(
        phrase(busy_prompt),
        "a turn is running; steer, queue, or cancel it"
    );
}

/// Oracle: §11.1 and M4 over any order. Each turn's end is published at most
/// once and only for the running turn; an older or unknown turn's end is
/// Stale. Every prompt starts a turn with a fresh identity, as the handover
/// mints one.
#[hegel::test]
fn any_event_order_ends_each_turn_once(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = SessionTurn::Idle;
    let mut started: Vec<TurnId> = Vec::new();
    let mut ended: Vec<TurnId> = Vec::new();
    let mut minted: u128 = 100;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let event = match case.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                minted += 1;
                SessionTurnEvent::PromptStarted { turn: turn(minted) }
            }
            1 => SessionTurnEvent::SteerRequested {
                agent_steers: case.draw(gs::booleans()),
            },
            2 => SessionTurnEvent::CancelRequested,
            _ => {
                let pick = usize::from(case.draw(gs::integers::<u8>().max_value(8)));
                SessionTurnEvent::TurnEnded {
                    turn: started.get(pick).copied().unwrap_or(turn(1)),
                }
            }
        };
        let before = state;
        let Ok((next, effects)) = SessionTurnMachine::transition(&state, event) else {
            continue;
        };
        if let SessionTurnEvent::PromptStarted { turn } = event {
            started.push(turn);
        }
        for effect in effects {
            match effect {
                SessionTurnEffect::PublishTurnEnded { turn } => {
                    assert!(matches!(
                        before,
                        SessionTurn::Running { turn: running } | SessionTurn::Cancelling { turn: running }
                            if running == turn
                    ));
                    assert!(!ended.contains(&turn), "a turn's end was published twice");
                    ended.push(turn);
                }
                SessionTurnEffect::InjectSteer { turn }
                | SessionTurnEffect::WriteCancel { turn } => {
                    assert_eq!(before, SessionTurn::Running { turn });
                }
            }
        }
        state = next;
    }
}
