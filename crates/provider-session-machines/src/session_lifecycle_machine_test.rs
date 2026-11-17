#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::check_state_machine_transitions;

fn generation(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

fn load(id: u128) -> LoadAttempt {
    LoadAttempt(uuid::Uuid::from_u128(id))
}

fn close(id: u128) -> CloseAttempt {
    CloseAttempt(uuid::Uuid::from_u128(id))
}

fn every_state() -> Vec<SessionLifecycle> {
    vec![
        SessionLifecycle::NotLoaded {
            last: generation(1),
        },
        SessionLifecycle::Loading {
            generation: generation(2),
            attempt: load(1),
            via: RestoreVia::Load,
        },
        SessionLifecycle::Live {
            generation: generation(2),
        },
        SessionLifecycle::Closing {
            generation: generation(2),
            from: CloseOrigin::Live,
            phase: ClosePhase::EndingTurn,
            attempt: close(1),
        },
        SessionLifecycle::Closing {
            generation: generation(2),
            from: CloseOrigin::Live,
            phase: ClosePhase::Freeing,
            attempt: close(1),
        },
        SessionLifecycle::Closing {
            generation: generation(2),
            from: CloseOrigin::Loading,
            phase: ClosePhase::Freeing,
            attempt: close(1),
        },
    ]
}

/// Events whose payloads match the fixture states, plus stale variants.
fn every_event() -> Vec<SessionLifecycleEvent> {
    use SessionLifecycleEvent as V;
    vec![
        V::CreateAdmitted,
        V::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(2),
        },
        V::LoadRequested {
            via: RestoreVia::Resume,
            attempt: load(2),
        },
        V::Loaded { attempt: load(1) },
        V::Loaded { attempt: load(9) },
        V::LoadFailed { attempt: load(1) },
        V::CloseRequested {
            attempt: close(2),
            turn_running: true,
        },
        V::CloseRequested {
            attempt: close(2),
            turn_running: false,
        },
        V::TurnEnded {
            generation: generation(2),
        },
        V::CloseDone { attempt: close(1) },
        V::CloseDone { attempt: close(9) },
        V::CloseFailed { attempt: close(1) },
        V::ActorExited {
            generation: generation(2),
        },
        V::ActorExited {
            generation: generation(1),
        },
        V::PromptRequested,
        V::Publish {
            generation: generation(2),
        },
        V::Publish {
            generation: generation(1),
        },
    ]
}

/// Oracle: the d1b specification's session table (M2). Every state × event
/// pair either takes exactly a declared edge or is refused, and every
/// declared edge is reached.
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let cases = every_state()
        .into_iter()
        .flat_map(|state| every_event().into_iter().map(move |event| (state, event)));

    // Act
    let checked = check_state_machine_transitions::<SessionLifecycleMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

/// Oracle: M3 and M4. A close ends NotLoaded and the next load starts a
/// newer generation; the closed run's late load result and publication are
/// Stale.
#[test]
fn a_closed_session_loads_again_and_ignores_its_old_run() {
    // Arrange
    let live = SessionLifecycle::Live {
        generation: generation(3),
    };

    // Act
    let (closing, _) = SessionLifecycleMachine::transition(
        &live,
        SessionLifecycleEvent::CloseRequested {
            attempt: close(5),
            turn_running: false,
        },
    )
    .expect("close is accepted");
    let (closed, effects) = SessionLifecycleMachine::transition(
        &closing,
        SessionLifecycleEvent::CloseDone { attempt: close(5) },
    )
    .expect("close completes");
    let stale_publish = SessionLifecycleMachine::transition(
        &closed,
        SessionLifecycleEvent::Publish {
            generation: generation(3),
        },
    );
    let (reloading, _) = SessionLifecycleMachine::transition(
        &closed,
        SessionLifecycleEvent::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(6),
        },
    )
    .expect("a closed session loads again");
    let stale_load = SessionLifecycleMachine::transition(
        &reloading,
        SessionLifecycleEvent::Loaded { attempt: load(1) },
    );

    // Assert
    assert_eq!(
        closed,
        SessionLifecycle::NotLoaded {
            last: generation(3)
        }
    );
    assert_eq!(
        effects,
        vec![SessionLifecycleEffect::PublishClosed {
            generation: generation(3)
        }]
    );
    assert_eq!(stale_publish, Err(TransitionError::Stale));
    assert!(matches!(
        reloading,
        SessionLifecycle::Loading { generation: next, .. } if next == generation(4)
    ));
    assert_eq!(stale_load, Err(TransitionError::Stale));
}

fn drawn_event(case: &hegel::TestCase) -> SessionLifecycleEvent {
    use SessionLifecycleEvent as V;
    use hegel::generators as gs;
    let id = u128::from(case.draw(gs::integers::<u8>().max_value(3)));
    let number = u64::from(case.draw(gs::integers::<u8>().max_value(6)));
    match case.draw(gs::integers::<u8>().max_value(11)) {
        0 => V::CreateAdmitted,
        1 => V::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(id),
        },
        2 => V::LoadRequested {
            via: RestoreVia::Resume,
            attempt: load(id),
        },
        3 => V::Loaded { attempt: load(id) },
        4 => V::LoadFailed { attempt: load(id) },
        5 => V::CloseRequested {
            attempt: close(id),
            turn_running: case.draw(gs::booleans()),
        },
        6 => V::TurnEnded {
            generation: generation(number),
        },
        7 => V::CloseDone { attempt: close(id) },
        8 => V::CloseFailed { attempt: close(id) },
        9 => V::ActorExited {
            generation: generation(number),
        },
        10 => V::PromptRequested,
        _ => V::Publish {
            generation: generation(number),
        },
    }
}

fn current_generation(state: &SessionLifecycle) -> SessionGeneration {
    match state {
        SessionLifecycle::NotLoaded { last } => *last,
        SessionLifecycle::Loading { generation, .. }
        | SessionLifecycle::Live { generation }
        | SessionLifecycle::Closing { generation, .. } => *generation,
    }
}

/// Oracle: M2, M3, M4 and M6 over any order of events. Every move taken is
/// declared; a refused event leaves the state unchanged; generations never go
/// back; an actor is registered and activation published only for the run
/// that just became Live; a hub publication is only for the current run;
/// Closed is published once per close and leaves NotLoaded.
#[hegel::test]
fn any_event_order_keeps_the_lifecycle_rules(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = SessionLifecycle::NotLoaded {
        last: generation(0),
    };
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        let event = drawn_event(&case);
        let before = state;
        let Ok((next, effects)) = SessionLifecycleMachine::transition(&state, event) else {
            continue;
        };
        let taken = Transition {
            from: before.state_tag(),
            event: event.event_tag(),
            to: next.state_tag(),
            terminal: next.is_terminal(),
        };
        assert!(
            SessionLifecycleMachine::transitions().contains(&taken),
            "undeclared move {taken:?}"
        );
        assert!(
            current_generation(&next).number() >= current_generation(&before).number(),
            "generation went back: {before:?} -> {next:?}"
        );
        // Terminal facts belong to their transition (design note §11.1).
        if taken.event == "close_done" {
            assert!(effects.contains(&SessionLifecycleEffect::PublishClosed {
                generation: current_generation(&before)
            }));
        }
        if taken.event == "actor_exited" {
            assert!(effects.contains(&SessionLifecycleEffect::PublishUnloaded {
                generation: current_generation(&before)
            }));
        }
        for effect in &effects {
            match effect {
                SessionLifecycleEffect::RegisterActor { generation }
                | SessionLifecycleEffect::PublishActivation { generation } => {
                    assert_eq!(
                        next,
                        SessionLifecycle::Live {
                            generation: *generation
                        }
                    );
                }
                SessionLifecycleEffect::PublishToHub { generation } => {
                    assert_eq!(*generation, current_generation(&before));
                    assert_eq!(next, before);
                }
                SessionLifecycleEffect::PublishClosed { generation } => {
                    assert_eq!(next, SessionLifecycle::NotLoaded { last: *generation });
                    assert!(matches!(before, SessionLifecycle::Closing { .. }));
                }
                SessionLifecycleEffect::PublishUnloaded { generation } => {
                    assert_eq!(
                        before,
                        SessionLifecycle::Live {
                            generation: *generation
                        }
                    );
                }
                SessionLifecycleEffect::StartRestore { .. }
                | SessionLifecycleEffect::EndLine { .. }
                | SessionLifecycleEffect::CancelTurn { .. }
                | SessionLifecycleEffect::StartFree { .. }
                | SessionLifecycleEffect::ReopenLine { .. } => {}
            }
        }
        state = next;
    }
}
