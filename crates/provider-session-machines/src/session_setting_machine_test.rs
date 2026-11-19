#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::check_state_machine_transitions;

fn value(name: &str) -> SettingValueId {
    SettingValueId(name.to_owned())
}

fn application(id: u128) -> SettingApplication {
    SettingApplication(uuid::Uuid::from_u128(id))
}

fn every_state() -> Vec<SessionSetting> {
    vec![
        SessionSetting::Current { value: value("a") },
        SessionSetting::Changing {
            to: value("b"),
            application: application(1),
            observed: value("a"),
        },
        SessionSetting::Unresolved {
            cause: UnresolvedCause::DefiniteFailure,
            observed: value("a"),
        },
        SessionSetting::Unresolved {
            cause: UnresolvedCause::OutcomeUnknown,
            observed: value("a"),
        },
    ]
}

fn every_event() -> Vec<SessionSettingEvent> {
    vec![
        SessionSettingEvent::SetRequested {
            to: value("c"),
            application: application(2),
        },
        SessionSettingEvent::Applied {
            application: application(1),
            value: value("b"),
        },
        SessionSettingEvent::Applied {
            application: application(9),
            value: value("z"),
        },
        SessionSettingEvent::Failed {
            application: application(1),
        },
        SessionSettingEvent::OutcomeUnknown {
            application: application(1),
        },
        SessionSettingEvent::Refused {
            application: application(1),
        },
        SessionSettingEvent::Lost,
        SessionSettingEvent::Observed { value: value("x") },
        SessionSettingEvent::Accept,
    ]
}

/// Oracle: design note §12.6 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let cases = every_state().into_iter().flat_map(|state| {
        every_event()
            .into_iter()
            .map(move |event| (state.clone(), event))
    });

    // Act
    let checked = check_state_machine_transitions::<SessionSettingMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

/// Oracle: §12.6 (R2-6). An observation during a change keeps the change and
/// its hold; accept is refused for an unknown outcome and while changing;
/// a refused change holds prompts until accepted.
#[test]
fn observations_never_clear_a_change_or_a_hold() {
    // Arrange
    let changing = SessionSetting::Changing {
        to: value("b"),
        application: application(1),
        observed: value("a"),
    };

    // Act
    let (observed, _) = SessionSettingMachine::transition(
        &changing,
        SessionSettingEvent::Observed { value: value("b") },
    )
    .expect("the observation is recorded");
    let (unknown, _) = SessionSettingMachine::transition(
        &observed,
        SessionSettingEvent::OutcomeUnknown {
            application: application(1),
        },
    )
    .expect("the change's outcome is unknown");
    let accept_unknown = SessionSettingMachine::transition(&unknown, SessionSettingEvent::Accept);
    let accept_changing = SessionSettingMachine::transition(&changing, SessionSettingEvent::Accept);
    let refused = SessionSetting::Unresolved {
        cause: UnresolvedCause::DefiniteFailure,
        observed: value("a"),
    };
    let (accepted, _) = SessionSettingMachine::transition(&refused, SessionSettingEvent::Accept)
        .expect("a refused change is accepted");

    // Assert
    assert!(matches!(observed, SessionSetting::Changing { .. }));
    assert_eq!(
        prompt_hold(&observed),
        Some("settings are changing; wait for them")
    );
    assert_eq!(
        accept_unknown,
        Err(TransitionError::Domain(
            SessionSettingRefusal::OutcomeUnknown
        ))
    );
    assert_eq!(
        SessionSettingRefusal::OutcomeUnknown.human_phrase(),
        "a setting change's outcome is unknown; set a value explicitly"
    );
    assert_eq!(
        accept_changing,
        Err(TransitionError::Domain(
            SessionSettingRefusal::ChangeInProgress
        ))
    );
    assert_eq!(
        prompt_hold(&refused),
        Some("a setting change was refused; accept the current value or set it again")
    );
    assert_eq!(accepted, SessionSetting::Current { value: value("a") });
}

/// Oracle: §12.6 over any order. Only the current change's own result leaves
/// Changing; an observation never resolves Unresolved; accept never clears an
/// unknown outcome.
#[hegel::test]
fn any_event_order_keeps_changes_and_holds(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = SessionSetting::Current { value: value("a") };
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let app = application(u128::from(case.draw(gs::integers::<u8>().max_value(3))));
        let name =
            value(["a", "b", "c"][usize::from(case.draw(gs::integers::<u8>().max_value(2)))]);
        let event = match case.draw(gs::integers::<u8>().max_value(7)) {
            0 => SessionSettingEvent::SetRequested {
                to: name,
                application: app,
            },
            1 => SessionSettingEvent::Applied {
                application: app,
                value: name,
            },
            2 => SessionSettingEvent::Failed { application: app },
            3 => SessionSettingEvent::OutcomeUnknown { application: app },
            4 => SessionSettingEvent::Observed { value: name },
            5 => SessionSettingEvent::Refused { application: app },
            6 => SessionSettingEvent::Lost,
            _ => SessionSettingEvent::Accept,
        };
        let before = state.clone();
        let Ok((next, _)) = SessionSettingMachine::transition(&state, event.clone()) else {
            continue;
        };
        if let SessionSetting::Changing { application, .. } = &before
            && !matches!(next, SessionSetting::Changing { .. })
        {
            assert!(
                matches!(
                    &event,
                    SessionSettingEvent::Applied { application: from, .. }
                        | SessionSettingEvent::Failed { application: from }
                        | SessionSettingEvent::Refused { application: from }
                        | SessionSettingEvent::OutcomeUnknown { application: from }
                        if from == application
                ) || matches!(&event, SessionSettingEvent::Lost)
            );
        }
        if matches!(event, SessionSettingEvent::Refused { .. }) {
            assert!(
                matches!(next, SessionSetting::Current { .. }),
                "a refusal of a kind that doesn't hold left a hold: {next:?}"
            );
        }
        if matches!(event, SessionSettingEvent::Lost) {
            assert!(matches!(
                next,
                SessionSetting::Unresolved {
                    cause: UnresolvedCause::OutcomeUnknown,
                    ..
                }
            ));
        }
        if matches!(before, SessionSetting::Unresolved { .. })
            && matches!(event, SessionSettingEvent::Observed { .. })
        {
            assert!(matches!(next, SessionSetting::Unresolved { .. }));
        }
        if matches!(
            before,
            SessionSetting::Unresolved {
                cause: UnresolvedCause::OutcomeUnknown,
                ..
            }
        ) {
            assert!(
                !matches!(event, SessionSettingEvent::Accept),
                "accept cleared an unknown outcome"
            );
        }
        state = next;
    }
}

/// Oracle: today's provider_session_setting_update.rs (the Lead's ruling on
/// 3b-5): an agent's definite refusal of mode, model or effort holds
/// nothing: the kind returns to the value it had before the change.
#[test]
fn a_refused_change_of_a_kind_that_does_not_hold_returns_to_its_value() {
    // Arrange
    let changing = SessionSetting::Changing {
        to: value("b"),
        application: application(1),
        observed: value("a"),
    };

    // Act
    let refused = SessionSettingMachine::transition(
        &changing,
        SessionSettingEvent::Refused {
            application: application(1),
        },
    );
    let stale = SessionSettingMachine::transition(
        &changing,
        SessionSettingEvent::Refused {
            application: application(9),
        },
    );

    // Assert
    assert_eq!(
        refused.map(|(next, _)| next),
        Ok(SessionSetting::Current { value: value("a") })
    );
    assert_eq!(stale, Err(TransitionError::Stale));
}

/// Oracle: today's set_setting (provider_client_operations.rs): a change
/// whose reply never came back holds the kind as an unknown outcome,
/// whatever it was before.
#[test]
fn a_lost_reply_holds_the_kind_as_an_unknown_outcome() {
    // Arrange
    let states = every_state();

    // Act
    let held: Vec<SessionSetting> = states
        .iter()
        .filter_map(|state| {
            SessionSettingMachine::transition(state, SessionSettingEvent::Lost)
                .ok()
                .map(|(next, _)| next)
        })
        .collect();

    // Assert
    assert_eq!(held.len(), states.len());
    assert!(held.iter().all(|state| matches!(
        state,
        SessionSetting::Unresolved {
            cause: UnresolvedCause::OutcomeUnknown,
            ..
        }
    )));
}
