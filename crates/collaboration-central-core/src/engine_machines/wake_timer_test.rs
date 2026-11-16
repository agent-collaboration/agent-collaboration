use super::*;
use crate::engine_machines::timer_test_support::*;
use central_engine_model::check_transitions;

#[hegel::test]
fn wake_sequences_are_deterministic_declared_and_finish_once(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = match case.draw(gs::integers::<u8>().max_value(2)) {
        0 => State::Active(wake()),
        1 => State::Done,
        _ => State::Disabled,
    };
    let mut fired = 0;
    for step in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(50)) {
        let now = time(case.draw(gs::integers::<i64>().min_value(0).max_value(10_080)));
        let event = match case.draw(gs::integers::<u8>().max_value(3)) {
            0 => Event::Due {
                occurrence: time(5),
            },
            1 => Event::Due {
                occurrence: time(case.draw(gs::integers::<i64>().min_value(0).max_value(10_080))),
            },
            2 => Event::Cancel,
            _ => Event::Discard,
        };
        match crate::engine::machine_property_support::checked_step::<WakeTimer>(
            &state,
            event,
            now,
            u64::from(step),
        ) {
            Ok((next, effects)) => {
                fired += effects.len();
                assert!(fired <= 1, "one wake can queue only one message");
                state = next;
            }
            Err(
                TransitionError::Domain(_)
                | TransitionError::NotAllowed { .. }
                | TransitionError::Stale,
            ) => {}
        }
    }
}

#[test]
fn every_wake_state_event_pair_matches_the_declared_edges() {
    let cases = [State::Active(wake()), State::Done, State::Disabled]
        .into_iter()
        .flat_map(|state| {
            [
                Event::Due {
                    occurrence: time(5),
                },
                Event::Cancel,
                Event::Discard,
            ]
            .into_iter()
            .map(move |event| (state.clone(), event))
        });
    assert_eq!(
        check_transitions::<WakeTimer>(cases, &mut context(5, &mut ReplayIds)),
        Ok(())
    );
}

#[test]
fn wake_effects_replay_identifiers_and_a_moved_or_finished_due_is_stale() {
    let state = State::Active(wake());
    let fire = || {
        WakeTimer::transition(
            &state,
            Event::Due {
                occurrence: time(5),
            },
            &mut context(4, &mut ReplayIds),
        )
        .unwrap()
    };
    let first = fire();
    let second = fire();
    assert!(matches!(first.0, State::Done));
    let [Effect::QueueMessage(first)] = first.1.as_slice() else {
        panic!("one wake message");
    };
    let [Effect::QueueMessage(second)] = second.1.as_slice() else {
        panic!("one wake message");
    };
    assert_eq!(first, second);
    assert_eq!(
        first.message_id.as_str(),
        "01890f2e-7b4c-7cc0-98c4-000000000010"
    );
    for state in [state, State::Done, State::Disabled] {
        assert!(matches!(
            WakeTimer::transition(
                &state,
                Event::Due {
                    occurrence: time(10)
                },
                &mut context(10, &mut ReplayIds)
            ),
            Err(TransitionError::Stale)
        ));
    }
}
