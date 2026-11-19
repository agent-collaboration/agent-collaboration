use super::*;
use crate::engine::machines::timer_test_support::{ReplayIds, context, time};
use central_engine_model::check_transitions;

fn session(name: &str) -> SessionRef {
    SessionRef {
        machine_id: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        session_id: name.to_owned().try_into().unwrap(),
    }
}

fn subject(sender_session: Option<SessionRef>) -> DirectMessageSubject {
    DirectMessageSubject {
        id: "01890f2e-7b4c-7cc0-98c4-0000000000a1".parse().unwrap(),
        sender_session,
        sender_request_id: "01890f2e-7b4c-7cc0-98c4-0000000000b1".parse().unwrap(),
        cancel_due_at: time(20),
    }
}

const REFUSED: mail::DeliveryAnswer = mail::DeliveryAnswer::Refused {
    reason: mail::MessageRefusal::UnknownSession,
};

fn outcomes(effects: &[Effect]) -> Vec<mail::MessageOutcome> {
    effects
        .iter()
        .map(|Effect::TellSender(notice)| {
            let mail::MessageBody::DeliveryOutcome {
                message_id,
                request_id,
                outcome,
            } = &notice.body
            else {
                panic!("a sender notice is a DeliveryOutcome body: {notice:?}");
            };
            let expected = subject(None);
            assert_eq!(message_id, &expected.id);
            assert_eq!(request_id, &expected.sender_request_id);
            assert_eq!(notice.target, session("sender"));
            assert_eq!(notice.sender, mail::MessageSender::Central);
            assert_eq!(notice.state, mail::MessageState::Queued);
            *outcome
        })
        .collect()
}

/// The design's invariants over any interleaving of the cancel mark, early or
/// moved due runs, and repeated Acks: never cancelled except at its mark; never
/// delivered after a cancel except by an accepted Ack, which tells the sender
/// "delivered after all"; once delivered or failed nothing changes; the sender
/// hears at most one notice per transition and only when it has a session.
#[hegel::test(test_cases = 400)]
fn a_direct_message_ends_once_and_is_cancelled_only_at_its_mark(case: hegel::TestCase) {
    use hegel::generators as gs;
    let told = case.draw(gs::booleans());
    let start = subject(told.then(|| session("sender")));
    let mut state = State::Queued(start.clone());
    let mut heard = Vec::new();
    let mut settled: Option<State> = None;
    for step in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(40)) {
        let now = time(case.draw(gs::integers::<i64>().min_value(0).max_value(60)));
        let event = match case.draw(gs::integers::<u8>().max_value(4)) {
            0 => Event::Due {
                occurrence: start.cancel_due_at,
            },
            1 => Event::Due {
                occurrence: time(case.draw(gs::integers::<i64>().min_value(0).max_value(60))),
            },
            2 | 3 => Event::Ack(mail::DeliveryAnswer::Accepted),
            _ => Event::Ack(REFUSED),
        };
        let before = state.clone();
        match crate::engine::machine_property_support::checked_step::<DirectMessage>(
            &state,
            event.clone(),
            now,
            u64::from(step),
        ) {
            Ok((next, effects)) => {
                let told_now = outcomes(&effects);
                assert!(
                    told || told_now.is_empty(),
                    "a person sender has no session to tell"
                );
                assert!(told_now.len() <= 1);
                if matches!(next, State::Cancelled(_)) && !matches!(before, State::Cancelled(_)) {
                    assert!(
                        matches!(event, Event::Due { occurrence } if occurrence == start.cancel_due_at),
                        "cancelled only by the due run at its own mark"
                    );
                }
                if matches!(before, State::Cancelled(_)) {
                    assert!(matches!(next, State::Delivered));
                    assert!(matches!(event, Event::Ack(mail::DeliveryAnswer::Accepted)));
                    if told {
                        assert_eq!(told_now, vec![mail::MessageOutcome::DeliveredAfterAll]);
                    }
                }
                if matches!(next, State::Delivered) && matches!(before, State::Queued(_)) {
                    assert!(told_now.is_empty(), "a normal delivery sends no outcome");
                }
                assert!(
                    settled.is_none(),
                    "a settled message cannot change: {settled:?}"
                );
                if next.is_terminal() {
                    settled = Some(next.clone());
                }
                heard.extend(told_now);
                state = next;
            }
            Err(TransitionError::Domain(DirectMessageRefusal::AlreadySettled)) => {
                assert!(matches!(event, Event::Ack(_)));
                assert!(!matches!(before, State::Queued(_)));
            }
            Err(TransitionError::Stale) => {
                assert!(matches!(event, Event::Due { .. }));
            }
            Err(other) => panic!("unexpected refusal {other:?}"),
        }
    }
    assert!(
        heard.len() <= 2,
        "at most cancelled then delivered after all: {heard:?}"
    );
}

#[test]
fn every_direct_message_state_event_pair_matches_the_declared_edges() {
    let states = [
        State::Queued(subject(Some(session("sender")))),
        State::Cancelled(subject(Some(session("sender")))),
        State::Delivered,
        State::Failed(mail::MessageFailureReason::UnknownSession),
    ];
    let cases = states.into_iter().flat_map(|state| {
        [
            Event::Due {
                occurrence: time(20),
            },
            Event::Ack(mail::DeliveryAnswer::Accepted),
            Event::Ack(REFUSED),
        ]
        .into_iter()
        .map(move |event| (state.clone(), event))
    });
    assert_eq!(
        check_transitions::<DirectMessage>(cases, &mut context(20, &mut ReplayIds)),
        Ok(())
    );
}

#[test]
fn the_cancel_mark_cancels_and_tells_the_sender_and_a_late_ack_delivers_after_all() {
    let queued = State::Queued(subject(Some(session("sender"))));
    let (cancelled, effects) = DirectMessage::transition(
        &queued,
        Event::Due {
            occurrence: time(20),
        },
        &mut context(20, &mut ReplayIds),
    )
    .unwrap();
    assert_eq!(
        outcomes(&effects),
        vec![mail::MessageOutcome::Cancelled {
            reason: mail::MessageCancellation::NotDeliveredWithin15Minutes
        }]
    );
    assert_eq!(DirectMessage::next_due(&cancelled), None);
    let (delivered, effects) = DirectMessage::transition(
        &cancelled,
        Event::Ack(mail::DeliveryAnswer::Accepted),
        &mut context(40, &mut ReplayIds),
    )
    .unwrap();
    assert_eq!(delivered, State::Delivered);
    assert_eq!(
        outcomes(&effects),
        vec![mail::MessageOutcome::DeliveredAfterAll]
    );
}

#[test]
fn a_refusal_fails_the_message_with_its_reason_and_a_moved_mark_is_stale() {
    let queued = State::Queued(subject(Some(session("sender"))));
    let (failed, effects) = DirectMessage::transition(
        &queued,
        Event::Ack(REFUSED),
        &mut context(3, &mut ReplayIds),
    )
    .unwrap();
    assert_eq!(
        failed,
        State::Failed(mail::MessageFailureReason::UnknownSession)
    );
    assert_eq!(
        outcomes(&effects),
        vec![mail::MessageOutcome::Refused {
            reason: mail::MessageRefusal::UnknownSession
        }]
    );
    assert!(matches!(
        DirectMessage::transition(
            &queued,
            Event::Due {
                occurrence: time(25)
            },
            &mut context(25, &mut ReplayIds)
        ),
        Err(TransitionError::Stale)
    ));
    let (_, silent) = DirectMessage::transition(
        &State::Queued(subject(None)),
        Event::Due {
            occurrence: time(20),
        },
        &mut context(20, &mut ReplayIds),
    )
    .unwrap();
    assert!(silent.is_empty(), "a person sender gets no notice");
}
