use super::*;
use state_machine_core::check_state_machine_transitions;

/// The Host keys the machine by its protocol identifiers; these tests use
/// strings in their place.
type Outcomes = UnknownOutcomes<String, String>;
type Machine = UncertaintyCellMachine<String, String>;

fn session(name: &str) -> String {
    format!("cursor-local/{name}")
}

fn operation(suffix: u8) -> String {
    format!("01a1e700-0000-7000-8000-0000000000{suffix:02x}")
}

/// Oracle: design note §14.1 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let clear = Outcomes::default();
    let holding_one = seeded([(&operation(1), Some(&session("b")))]);
    let holding_two = seeded([
        (&operation(1), Some(&session("b"))),
        (&operation(2), Some(&session("c"))),
    ]);
    let events = [
        UncertaintyEvent::BecameUnknown {
            operation: operation(3),
            target: Some(session("b")),
        },
        UncertaintyEvent::Reconciled {
            operation: operation(1),
        },
        UncertaintyEvent::AdmissionCheck {
            target: Some(session("z")),
        },
    ];
    let cases = [clear, holding_one, holding_two]
        .into_iter()
        .flat_map(|state| {
            events
                .clone()
                .into_iter()
                .map(move |event| (state.clone(), event))
        });

    // Act
    let checked = check_state_machine_transitions::<Machine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

fn admits(state: &Outcomes, target: Option<&String>) -> bool {
    Machine::transition(
        state,
        UncertaintyEvent::AdmissionCheck {
            target: target.cloned(),
        },
    )
    .is_ok()
}

fn apply(state: Outcomes, event: UncertaintyEvent<String, String>) -> Outcomes {
    Machine::transition(&state, event)
        .map(|(next, _)| next)
        .unwrap_or(state)
}

/// Oracle: the Reviewer's R4-1 counterexample. With C unknown and A live on
/// B's session, then A becoming unknown and C reconciling, B is refused at
/// every point: one cell read can't mix moments.
#[test]
fn an_admission_is_refused_at_every_point_of_the_a_c_order() {
    // Arrange
    let b = session("b");
    let c_unknown = seeded([(&operation(0x0c), Some(&b))]);

    // Act
    let before_a = admits(&c_unknown, Some(&b));
    let both = apply(
        c_unknown,
        UncertaintyEvent::BecameUnknown {
            operation: operation(0x0a),
            target: Some(b.clone()),
        },
    );
    let while_both = admits(&both, Some(&b));
    let only_a = apply(
        both,
        UncertaintyEvent::Reconciled {
            operation: operation(0x0c),
        },
    );
    let after_c = admits(&only_a, Some(&b));

    // Assert
    assert_eq!((before_a, while_both, after_c), (false, false, false));
}

/// Oracle: today's scan treats a targetless unknown row as binding-wide.
#[test]
fn a_targetless_unknown_blocks_every_session_until_reconciled() {
    // Arrange
    let targetless = seeded([(&operation(7), None)]);

    // Act
    let refused = Machine::transition(
        &targetless,
        UncertaintyEvent::AdmissionCheck {
            target: Some(session("other")),
        },
    );
    let cleared = apply(
        targetless,
        UncertaintyEvent::Reconciled {
            operation: operation(7),
        },
    );

    // Assert
    let Err(TransitionError::Domain(refusal)) = refused else {
        panic!("a targetless unknown must block: {refused:?}");
    };
    assert_eq!(
        refusal,
        UncertaintyRefusal::EarlierOutcomeUnknown {
            operations: vec![operation(7)],
        }
    );
    assert!(admits(&cleared, Some(&session("other"))));
}

/// Oracle: §14.1's linearizability over any order of writes and checks: each
/// admission answer equals a plain reference model's answer at that moment.
#[hegel::test]
fn every_admission_matches_the_reference_set(case: hegel::TestCase) {
    use hegel::generators as gs;
    let targets = [Some(session("b")), Some(session("c")), None];
    let mut state = Outcomes::default();
    let mut model: Vec<(String, Option<String>)> = Vec::new();
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let op = operation(case.draw(gs::integers::<u8>().max_value(5)));
        let target = targets[usize::from(case.draw(gs::integers::<u8>().max_value(2)))].clone();
        match case.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                state = apply(
                    state,
                    UncertaintyEvent::BecameUnknown {
                        operation: op.clone(),
                        target: target.clone(),
                    },
                );
                model.push((op, target));
            }
            1 => {
                state = apply(
                    state,
                    UncertaintyEvent::Reconciled {
                        operation: op.clone(),
                    },
                );
                model.retain(|(held, _)| held != &op);
            }
            _ => {
                // A create asks with no target: only binding-wide unknowns
                // block it.
                let asking = target;
                let expected_blocked = model.iter().any(|(_, held)| {
                    held.is_none() || (asking.is_some() && held.as_ref() == asking.as_ref())
                });
                assert_eq!(admits(&state, asking.as_ref()), !expected_blocked);
            }
        }
    }
}

/// Oracle: today's scan for an admission with no target yet (a create):
/// only binding-wide unknowns block it; a session's unknown blocks that
/// session alone.
#[test]
fn a_create_is_blocked_only_by_binding_wide_unknowns() {
    // Arrange
    let session_unknown = seeded([(&operation(1), Some(&session("b")))]);
    let binding_unknown = seeded([(&operation(2), None)]);

    // Act
    let create_past_session_unknown = admits(&session_unknown, None);
    let create_past_binding_unknown = admits(&binding_unknown, None);

    // Assert
    assert!(
        create_past_session_unknown,
        "a session's unknown doesn't block a create"
    );
    assert!(!create_past_binding_unknown, "a binding-wide unknown does");
}
