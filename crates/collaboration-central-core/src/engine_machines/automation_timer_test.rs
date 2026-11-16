use super::*;
use crate::engine_machines::timer_test_support::*;
use central_engine_model::check_transitions;

#[hegel::test]
fn automation_sequences_never_repeat_an_occurrence_and_catch_up_once(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = match case.draw(gs::integers::<u8>().max_value(3)) {
        0 => State::Active(automation(true)),
        1 => State::Active(automation(false)),
        2 => State::Done {
            evaluated: Some(time(5)),
        },
        _ => State::Disabled { evaluated: None },
    };
    let mut consumed = std::collections::BTreeSet::new();
    for step in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(50)) {
        let now = time(case.draw(gs::integers::<i64>().min_value(0).max_value(10_080)));
        let current = AutomationTimer::next_due(&state).unwrap_or(time(5));
        let event = match case.draw(gs::integers::<u8>().max_value(3)) {
            0 => Event::Due {
                occurrence: current,
            },
            1 => Event::Due {
                occurrence: time(case.draw(gs::integers::<i64>().min_value(0).max_value(10_080))),
            },
            2 => Event::Disable,
            _ => Event::Discard,
        };
        let selected = match &event {
            Event::Due { occurrence } => Some(*occurrence),
            Event::Disable | Event::Discard => None,
        };
        match crate::engine::machine_property_support::checked_step::<AutomationTimer>(
            &state,
            event,
            now,
            u64::from(step),
        ) {
            Ok((next, effects)) => {
                let messages = effects
                    .iter()
                    .filter(|effect| matches!(effect, Effect::QueueMessage(_)))
                    .count();
                if let Some(occurrence) = selected {
                    assert_eq!(messages, 1, "one catch-up queues exactly one message");
                    assert!(
                        consumed.insert(occurrence),
                        "an occurrence cannot fire twice"
                    );
                    let through = occurrence.max(now);
                    match &next {
                        State::Active(row) => {
                            assert_eq!(row.evaluated, Some(through));
                            let expected = EngineTime::from_timestamp(
                                (through.timestamp().div_euclid(300) + 1) * 300,
                                0,
                            )
                            .unwrap();
                            assert_eq!(
                                row.due,
                                Some(expected),
                                "overdue interval skips missed occurrences once"
                            );
                        }
                        State::Done { evaluated } => assert_eq!(*evaluated, Some(through)),
                        State::Disabled { .. } => panic!("Due never disables a timer"),
                    }
                } else {
                    assert_eq!(messages, 0);
                }
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
fn every_automation_state_event_pair_matches_the_declared_edges() {
    let cases = [
        State::Active(automation(true)),
        State::Active(automation(false)),
        State::Done {
            evaluated: Some(time(5)),
        },
        State::Disabled { evaluated: None },
    ]
    .into_iter()
    .flat_map(|state| {
        [
            Event::Due {
                occurrence: time(5),
            },
            Event::Disable,
            Event::Discard,
        ]
        .into_iter()
        .map(move |event| (state.clone(), event))
    });
    assert_eq!(
        check_transitions::<AutomationTimer>(cases, &mut context(5, &mut ReplayIds)),
        Ok(())
    );
}

#[test]
fn early_and_overdue_occurrences_are_consumed_once_and_old_occurrences_are_stale() {
    for (now, through, next_due) in [(4, 5, 10), (165, 165, 170)] {
        let state = State::Active(automation(true));
        let (next, effects) = AutomationTimer::transition(
            &state,
            Event::Due {
                occurrence: time(5),
            },
            &mut context(now, &mut ReplayIds),
        )
        .unwrap();
        let State::Active(row) = &next else {
            panic!("interval stays active");
        };
        assert_eq!(row.evaluated, Some(time(through)));
        assert_eq!(row.due, Some(time(next_due)));
        assert!(
            matches!(effects.as_slice(),[Effect::QueueMessage(_),Effect::ScheduleAt(at)] if *at==time(next_due))
        );
        assert!(matches!(
            AutomationTimer::transition(
                &next,
                Event::Due {
                    occurrence: time(5)
                },
                &mut context(now, &mut ReplayIds)
            ),
            Err(TransitionError::Stale)
        ));
        let (_, replayed) = AutomationTimer::transition(
            &state,
            Event::Due {
                occurrence: time(5),
            },
            &mut context(now, &mut ReplayIds),
        )
        .unwrap();
        let Effect::QueueMessage(first) = &effects[0] else {
            panic!("message");
        };
        let Effect::QueueMessage(second) = &replayed[0] else {
            panic!("message");
        };
        assert_eq!(first, second);
    }
}
