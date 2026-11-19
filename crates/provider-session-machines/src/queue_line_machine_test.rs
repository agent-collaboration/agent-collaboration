#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::check_state_machine_transitions;

fn generation(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

/// Oracle: design note §11.2 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let states = [
        QueueLine::Open {
            epoch: generation(2),
            next_sequence: 3,
        },
        QueueLine::Ended {
            epoch: generation(2),
            reason: NotSentReason::SessionClosed,
            next_sequence: 3,
        },
    ];
    let events = [
        QueueLineEvent::Enqueue {
            epoch: generation(2),
        },
        QueueLineEvent::Enqueue {
            epoch: generation(1),
        },
        QueueLineEvent::End {
            reason: NotSentReason::SessionClosed,
        },
        QueueLineEvent::Reopen {
            epoch: generation(2),
        },
        QueueLineEvent::OpenFresh {
            epoch: generation(3),
        },
        QueueLineEvent::OpenFresh {
            epoch: generation(1),
        },
    ];
    let cases = states
        .into_iter()
        .flat_map(|state| events.into_iter().map(move |event| (state, event)));

    // Act
    let checked = check_state_machine_transitions::<QueueLineMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

/// Oracle: §11.2's line lifetime. An ended line refuses sends with its reason;
/// a failed close reopens it; the next run's fresh line refuses the old run's
/// enqueue as Stale.
#[test]
fn a_line_ends_reopens_and_is_replaced_by_the_next_run() {
    // Arrange
    let open = QueueLine::Open {
        epoch: generation(2),
        next_sequence: 0,
    };

    // Act
    let (ended, withdraw) = QueueLineMachine::transition(
        &open,
        QueueLineEvent::End {
            reason: NotSentReason::SessionClosed,
        },
    )
    .expect("the line ends");
    let refused = QueueLineMachine::transition(
        &ended,
        QueueLineEvent::Enqueue {
            epoch: generation(2),
        },
    );
    let (reopened, _) = QueueLineMachine::transition(
        &ended,
        QueueLineEvent::Reopen {
            epoch: generation(2),
        },
    )
    .expect("a failed close reopens the line");
    let (fresh, _) = QueueLineMachine::transition(
        &ended,
        QueueLineEvent::OpenFresh {
            epoch: generation(3),
        },
    )
    .expect("the next run opens a fresh line");
    let old_enqueue = QueueLineMachine::transition(
        &fresh,
        QueueLineEvent::Enqueue {
            epoch: generation(2),
        },
    );

    // Assert
    assert_eq!(
        withdraw,
        vec![QueueLineEffect::WithdrawAll {
            reason: NotSentReason::SessionClosed
        }]
    );
    assert_eq!(
        refused,
        Err(TransitionError::Domain(QueueLineRefusal::Ended(
            NotSentReason::SessionClosed
        )))
    );
    assert!(matches!(reopened, QueueLine::Open { epoch, .. } if epoch == generation(2)));
    assert_eq!(old_enqueue, Err(TransitionError::Stale));
}

/// Oracle: §11.2 over any order. Sequences in one epoch never repeat; an
/// epoch never goes back; only an open line admits.
#[hegel::test]
fn any_event_order_keeps_sequences_unique_and_epochs_forward(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = QueueLine::Open {
        epoch: generation(1),
        next_sequence: 0,
    };
    let mut admitted: Vec<(u64, u64)> = Vec::new();
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let epoch = generation(u64::from(case.draw(gs::integers::<u8>().max_value(4))));
        let event = match case.draw(gs::integers::<u8>().max_value(3)) {
            0 => QueueLineEvent::Enqueue { epoch },
            1 => QueueLineEvent::End {
                reason: NotSentReason::SessionClosed,
            },
            2 => QueueLineEvent::Reopen { epoch },
            _ => QueueLineEvent::OpenFresh { epoch },
        };
        let before = state;
        let Ok((next, effects)) = QueueLineMachine::transition(&state, event) else {
            continue;
        };
        let (QueueLine::Open { epoch: was, .. } | QueueLine::Ended { epoch: was, .. }) = before;
        let (QueueLine::Open { epoch: now, .. } | QueueLine::Ended { epoch: now, .. }) = next;
        assert!(now.number() >= was.number(), "epoch went back");
        for effect in effects {
            match effect {
                QueueLineEffect::Admit { sequence } => {
                    assert!(matches!(before, QueueLine::Open { .. }));
                    assert!(
                        !admitted.contains(&(now.number(), sequence)),
                        "sequence repeated in its epoch"
                    );
                    admitted.push((now.number(), sequence));
                }
                QueueLineEffect::WithdrawAll { .. } => {
                    assert!(matches!(next, QueueLine::Ended { .. }));
                }
            }
        }
        state = next;
    }
}

/// Oracle: §11.2's "an old entry can't enter the new run's line", held by the
/// numbering: a fresh run's line continues the session's sequence numbers,
/// so a message number held from an earlier run never names one in the new
/// run.
#[test]
fn a_fresh_runs_line_continues_the_sequence_numbers() {
    // Arrange
    let first = SessionGeneration::from_number(1);
    let second = SessionGeneration::from_number(2);
    let (open, _) = QueueLineMachine::transition(
        &QueueLine::Ended {
            epoch: SessionGeneration::UNTRACKED,
            reason: NotSentReason::SessionClosed,
            next_sequence: 0,
        },
        QueueLineEvent::OpenFresh { epoch: first },
    )
    .expect("the first run's line opens");
    let (after_one, _) =
        QueueLineMachine::transition(&open, QueueLineEvent::Enqueue { epoch: first })
            .expect("one message");

    // Act
    let (fresh, _) =
        QueueLineMachine::transition(&after_one, QueueLineEvent::OpenFresh { epoch: second })
            .expect("the next run's line opens");
    let (_, admitted) =
        QueueLineMachine::transition(&fresh, QueueLineEvent::Enqueue { epoch: second })
            .expect("a message in the new run");

    // Assert
    assert_eq!(admitted, vec![QueueLineEffect::Admit { sequence: 1 }]);
}
