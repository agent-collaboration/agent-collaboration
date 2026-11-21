use super::*;
use central_engine_model::{IdSource, check_transitions};

/// Hands out the supplied identifiers in order, as production's UUIDv7 source would.
struct SuppliedIds(Vec<Uuid>);
impl IdSource for SuppliedIds {
    fn next_id(&mut self) -> Uuid {
        self.0.remove(0)
    }
}

fn id(suffix: u64) -> Uuid {
    format!("01890f2e-7b4c-7cc0-98c4-{suffix:012x}")
        .parse()
        .unwrap()
}
fn time(second: i64) -> EngineTime {
    EngineTime::from_timestamp(second, 0).unwrap()
}
const FIRST_CONNECTION: u64 = 0xa01;
const SECOND_CONNECTION: u64 = 0xa02;
const IN_FLIGHT_SEND: u64 = 0x501;
const OTHER_SEND: u64 = 0x502;
const MINTED_SEND: u64 = 0x503;

fn in_flight() -> State {
    State::InFlight {
        send_id: id(IN_FLIGHT_SEND),
        connection: id(FIRST_CONNECTION),
    }
}
fn answer(send: u64, connection: u64, answer: RouterAnswer) -> Event {
    Event::Answer {
        send_id: id(send),
        connection: id(connection),
        answer,
    }
}
fn step(
    state: &State,
    event: Event,
) -> Result<(State, Vec<Effect>), TransitionError<OwedReplyRefusal>> {
    OwedReply::transition(
        state,
        event,
        &mut Context {
            now: time(300),
            ids: &mut SuppliedIds(vec![id(MINTED_SEND)]),
        },
    )
}
fn every_state() -> [State; 5] {
    [
        State::Waiting,
        in_flight(),
        State::Done {
            finish: Finish::Delivered,
        },
        State::Done {
            finish: Finish::Refused,
        },
        State::Expired,
    ]
}
fn every_event() -> Vec<Event> {
    let mut events = vec![
        Event::Send {
            connection: id(FIRST_CONNECTION),
        },
        Event::Reconnected {
            connection: id(SECOND_CONNECTION),
        },
        Event::RetentionEnded,
    ];
    for reply in [RouterAnswer::Accepted, RouterAnswer::RefusedForGood] {
        events.push(answer(IN_FLIGHT_SEND, FIRST_CONNECTION, reply));
        events.push(answer(OTHER_SEND, FIRST_CONNECTION, reply));
        events.push(answer(IN_FLIGHT_SEND, SECOND_CONNECTION, reply));
    }
    events
}

#[test]
fn every_owed_reply_state_event_pair_matches_the_declared_edges() {
    let cases = every_state().into_iter().flat_map(|state| {
        every_event()
            .into_iter()
            .map(move |event| (state.clone(), event))
    });
    let ids: Vec<Uuid> = (0..100).map(|n| id(0x1000 + n)).collect();
    assert_eq!(
        check_transitions::<OwedReply>(
            cases,
            &mut Context {
                now: time(300),
                ids: &mut SuppliedIds(ids),
            }
        ),
        Ok(())
    );
    for state in every_state() {
        assert_eq!(OwedReply::next_due(&state), None);
    }
}

#[test]
fn owed_reply_each_arrow_produces_its_exact_effect() {
    assert_eq!(
        step(
            &State::Waiting,
            Event::Send {
                connection: id(FIRST_CONNECTION)
            }
        ),
        Ok((
            State::InFlight {
                send_id: id(MINTED_SEND),
                connection: id(FIRST_CONNECTION),
            },
            vec![Effect::SendItem {
                send_id: id(MINTED_SEND),
                connection: id(FIRST_CONNECTION),
            }]
        ))
    );
    assert_eq!(
        step(
            &in_flight(),
            answer(IN_FLIGHT_SEND, FIRST_CONNECTION, RouterAnswer::Accepted)
        ),
        Ok((
            State::Done {
                finish: Finish::Delivered
            },
            vec![Effect::Finish(Finish::Delivered)]
        ))
    );
    assert_eq!(
        step(
            &in_flight(),
            answer(
                IN_FLIGHT_SEND,
                FIRST_CONNECTION,
                RouterAnswer::RefusedForGood
            )
        ),
        Ok((
            State::Done {
                finish: Finish::Refused
            },
            vec![Effect::Finish(Finish::Refused)]
        ))
    );
    assert_eq!(
        step(
            &in_flight(),
            Event::Reconnected {
                connection: id(SECOND_CONNECTION)
            }
        ),
        Ok((State::Waiting, vec![]))
    );
    for state in [State::Waiting, in_flight()] {
        assert_eq!(
            step(&state, Event::RetentionEnded),
            Ok((State::Expired, vec![]))
        );
    }
}

#[test]
fn owed_reply_refuses_late_answers_double_sends_and_finished_work() {
    for reply in [RouterAnswer::Accepted, RouterAnswer::RefusedForGood] {
        assert_eq!(
            step(&in_flight(), answer(OTHER_SEND, FIRST_CONNECTION, reply)),
            Err(TransitionError::Stale),
            "an answer to an older send changes nothing"
        );
        assert_eq!(
            step(
                &in_flight(),
                answer(IN_FLIGHT_SEND, SECOND_CONNECTION, reply)
            ),
            Err(TransitionError::Stale),
            "an answer from another connection changes nothing"
        );
        assert_eq!(
            step(
                &State::Waiting,
                answer(IN_FLIGHT_SEND, FIRST_CONNECTION, reply)
            ),
            Err(TransitionError::Stale)
        );
    }
    assert_eq!(
        step(
            &in_flight(),
            Event::Send {
                connection: id(SECOND_CONNECTION)
            }
        ),
        Err(TransitionError::Domain(OwedReplyRefusal::AlreadyInFlight))
    );
    assert_eq!(
        step(
            &State::Waiting,
            Event::Reconnected {
                connection: id(SECOND_CONNECTION)
            }
        ),
        Err(TransitionError::Stale)
    );
    for state in [
        State::Done {
            finish: Finish::Delivered,
        },
        State::Done {
            finish: Finish::Refused,
        },
        State::Expired,
    ] {
        for event in every_event() {
            assert_eq!(step(&state, event), Err(TransitionError::Stale));
        }
    }
}

#[test]
fn owed_reply_same_inputs_time_and_ids_give_the_same_result() {
    for state in every_state() {
        for event in every_event() {
            let run = || {
                OwedReply::transition(
                    &state,
                    event.clone(),
                    &mut Context {
                        now: time(300),
                        ids: &mut SuppliedIds(vec![id(MINTED_SEND), id(MINTED_SEND + 1)]),
                    },
                )
            };
            assert_eq!(run(), run());
        }
    }
}

#[hegel::test]
fn owed_reply_sequences_leave_refusals_unchanged_and_finish_once(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = match case.draw(gs::integers::<u8>().max_value(4)) {
        0 => State::Waiting,
        1 => in_flight(),
        2 => State::Done {
            finish: Finish::Delivered,
        },
        3 => State::Done {
            finish: Finish::Refused,
        },
        _ => State::Expired,
    };
    let mut finishes = 0;
    for step in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(50)) {
        let now = time(case.draw(gs::integers::<i64>().min_value(0).max_value(604_800)));
        let (current_send, current_connection) = match &state {
            State::InFlight {
                send_id,
                connection,
            } => (*send_id, *connection),
            State::Waiting | State::Done { .. } | State::Expired => {
                (id(IN_FLIGHT_SEND), id(FIRST_CONNECTION))
            }
        };
        let connection = if case.draw(gs::booleans()) {
            current_connection
        } else {
            id(SECOND_CONNECTION)
        };
        let event = match case.draw(gs::integers::<u8>().max_value(4)) {
            0 => Event::Send { connection },
            1 | 2 => Event::Answer {
                send_id: if case.draw(gs::booleans()) {
                    current_send
                } else {
                    id(OTHER_SEND)
                },
                connection,
                answer: if case.draw(gs::booleans()) {
                    RouterAnswer::Accepted
                } else {
                    RouterAnswer::RefusedForGood
                },
            },
            3 => Event::Reconnected { connection },
            _ => Event::RetentionEnded,
        };
        let before = state.clone();
        match central_engine_test_support::checked_step::<OwedReply>(
            &state,
            event,
            now,
            u64::from(step),
        ) {
            Ok((next, effects)) => {
                finishes += effects
                    .iter()
                    .filter(|effect| matches!(effect, Effect::Finish(_)))
                    .count();
                assert!(finishes <= 1, "an owed reply finishes at most once");
                if before.is_terminal() {
                    assert_eq!(next, before, "finished work stays finished");
                }
                state = next;
            }
            Err(
                TransitionError::Stale
                | TransitionError::Domain(_)
                | TransitionError::NotAllowed { .. },
            ) => {
                // A refusal carries no effects (the error arm has none) and installs
                // nothing; checked_step already asserts the input was not mutated.
            }
        }
        if before.is_terminal() {
            assert!(state.is_terminal(), "finished work stays finished");
        }
    }
}
