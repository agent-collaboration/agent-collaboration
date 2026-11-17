use super::*;
use state_machine_core::check_state_machine_transitions;

fn attempt(id: u128) -> DispatchAttempt {
    DispatchAttempt(uuid::Uuid::from_u128(id))
}

fn every_state() -> Vec<OperationRecord> {
    vec![
        OperationRecord::Reserving,
        OperationRecord::Dropped,
        OperationRecord::Admitted { queued: false },
        OperationRecord::Admitted { queued: true },
        OperationRecord::MayHaveDispatched {
            attempt: attempt(1),
        },
        OperationRecord::SettlingUnknown {
            attempt: attempt(1),
        },
        OperationRecord::Settled {
            effect: SettledEffect::Unknown,
            reconciliation: Reconciliation::Unresolved,
        },
        OperationRecord::Settled {
            effect: SettledEffect::Applied,
            reconciliation: Reconciliation::Confirmed,
        },
    ]
}

fn every_event() -> Vec<OperationRecordEvent> {
    use OperationRecordEvent as V;
    vec![
        V::DurableInserted,
        V::DurableConflict,
        V::DurableInsertFailed,
        V::Queued,
        V::DispatchMarked {
            attempt: attempt(2),
        },
        V::Undispatched {
            attempt: attempt(1),
        },
        V::NotSent,
        V::Completed {
            attempt: attempt(1),
            ending: AttemptEnding::Applied,
        },
        V::Completed {
            attempt: attempt(1),
            ending: AttemptEnding::Refused,
        },
        V::Completed {
            attempt: attempt(1),
            ending: AttemptEnding::Unknown,
        },
        V::UncertaintyRecorded {
            attempt: attempt(1),
        },
        V::Reconciled {
            reconciliation: Reconciliation::Confirmed,
        },
    ]
}

/// Oracle: design note §11.5, §12.5, §15.3, §16 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let cases = every_state()
        .into_iter()
        .flat_map(|state| every_event().into_iter().map(move |event| (state, event)));

    // Act
    let checked = check_state_machine_transitions::<OperationRecordMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

fn run(events: &[OperationRecordEvent]) -> (OperationRecord, Vec<OperationRecordEffect>) {
    let mut state = OperationRecord::Reserving;
    let mut effects = Vec::new();
    for event in events {
        if let Ok((next, more)) = OperationRecordMachine::transition(&state, *event) {
            state = next;
            effects.extend(more);
        }
    }
    (state, effects)
}

/// Oracle: §12.5 (R2-5). Two attempts at the same moment stay distinct: after
/// an undispatched rollback and a second attempt, the first attempt's late
/// completion changes nothing.
#[test]
fn a_late_completion_from_an_earlier_attempt_is_stale() {
    // Arrange
    use OperationRecordEvent as V;
    let events = [
        V::DurableInserted,
        V::DispatchMarked {
            attempt: attempt(1),
        },
        V::Undispatched {
            attempt: attempt(1),
        },
        V::DispatchMarked {
            attempt: attempt(2),
        },
        V::Completed {
            attempt: attempt(1),
            ending: AttemptEnding::Applied,
        },
    ];

    // Act
    let (state, effects) = run(&events);

    // Assert
    assert_eq!(
        state,
        OperationRecord::MayHaveDispatched {
            attempt: attempt(2)
        }
    );
    assert_eq!(effects, vec![OperationRecordEffect::AnswerAdmitted]);
}

/// Oracle: §16 and §14.1. A definite refusal settles None with no uncertainty;
/// an unknown ending records uncertainty while still live, then settles
/// Unresolved; reconciliation clears it.
#[test]
fn definite_refusals_settle_none_and_unknown_endings_record_uncertainty_first() {
    // Arrange
    use OperationRecordEvent as V;
    let dispatched = [
        V::DurableInserted,
        V::DispatchMarked {
            attempt: attempt(1),
        },
    ];

    // Act
    let (refused, refused_effects) = run(&[
        &dispatched[..],
        &[V::Completed {
            attempt: attempt(1),
            ending: AttemptEnding::Refused,
        }],
    ]
    .concat());
    let (reconciled, unknown_effects) = run(&[
        &dispatched[..],
        &[
            V::Completed {
                attempt: attempt(1),
                ending: AttemptEnding::Unknown,
            },
            V::UncertaintyRecorded {
                attempt: attempt(1),
            },
            V::Reconciled {
                reconciliation: Reconciliation::Confirmed,
            },
        ],
    ]
    .concat());

    // Assert
    assert_eq!(
        refused,
        OperationRecord::Settled {
            effect: SettledEffect::None,
            reconciliation: Reconciliation::Confirmed
        }
    );
    assert!(!refused_effects.contains(&OperationRecordEffect::RecordUncertainty));
    assert_eq!(
        unknown_effects,
        vec![
            OperationRecordEffect::AnswerAdmitted,
            OperationRecordEffect::RecordUncertainty,
            OperationRecordEffect::WriteTerminal(SettledEffect::Unknown),
            OperationRecordEffect::ClearUncertainty,
        ]
    );
    assert_eq!(
        reconciled,
        OperationRecord::Settled {
            effect: SettledEffect::Unknown,
            reconciliation: Reconciliation::Confirmed
        }
    );
}

/// Oracle: R15 and §14.1 over any order. A terminal record never moves; the
/// terminal row is written at most once; uncertainty is recorded before the
/// Unknown row and cleared only after reconciliation; every recorded
/// uncertainty is cleared at most once.
#[hegel::test]
fn any_event_order_writes_one_terminal_row_and_orders_uncertainty(case: hegel::TestCase) {
    use OperationRecordEvent as V;
    use hegel::generators as gs;
    let mut state = OperationRecord::Reserving;
    let mut terminal_writes = 0;
    let mut recorded = 0;
    let mut cleared = 0;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let id = attempt(u128::from(case.draw(gs::integers::<u8>().max_value(2))));
        let event = match case.draw(gs::integers::<u8>().max_value(9)) {
            0 => V::DurableInserted,
            1 => V::DurableConflict,
            2 => V::Queued,
            3 => V::DispatchMarked { attempt: id },
            4 => V::Undispatched { attempt: id },
            5 => V::NotSent,
            6 => V::Completed {
                attempt: id,
                ending: [
                    AttemptEnding::Applied,
                    AttemptEnding::Refused,
                    AttemptEnding::Unknown,
                ][usize::from(case.draw(gs::integers::<u8>().max_value(2)))],
            },
            7 => V::UncertaintyRecorded { attempt: id },
            8 => V::Reconciled {
                reconciliation: Reconciliation::NotReconcilable,
            },
            _ => V::DurableInsertFailed,
        };
        let before = state;
        let Ok((next, effects)) = OperationRecordMachine::transition(&state, event) else {
            continue;
        };
        assert!(!before.is_terminal(), "a terminal record moved: {before:?}");
        for effect in effects {
            match effect {
                OperationRecordEffect::WriteTerminal(SettledEffect::Unknown) => {
                    terminal_writes += 1;
                    assert_eq!(
                        recorded, 1,
                        "Unknown row written before uncertainty was recorded"
                    );
                }
                OperationRecordEffect::WriteTerminal(
                    SettledEffect::None | SettledEffect::Applied,
                ) => {
                    terminal_writes += 1;
                }
                OperationRecordEffect::RecordUncertainty => recorded += 1,
                OperationRecordEffect::ClearUncertainty => {
                    cleared += 1;
                    assert!(recorded >= cleared);
                }
                OperationRecordEffect::AnswerAdmitted
                | OperationRecordEffect::AnswerFromExistingRow
                | OperationRecordEffect::AnswerNotRecorded => {}
            }
        }
        state = next;
    }
    assert!(
        terminal_writes <= 1,
        "terminal row written {terminal_writes} times"
    );
    assert!(cleared <= 1);
}
