use super::*;
use state_machine_core::check_state_machine_transitions;

fn at(seconds: u64) -> ClockInstant {
    ClockInstant::ZERO.after(Duration::from_secs(seconds))
}

fn input(id: u128) -> InputId {
    InputId::new(format!("warm-input-{id}")).expect("input ID")
}

const CADENCE: WarmCadence = WarmCadence {
    every: Duration::from_secs(240),
    keep_warm_for: Duration::from_secs(3600),
};

/// Oracle: design note §3.7 (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let states = vec![
        CacheWarm::Cold {
            last_real_end: Some(at(0)),
        },
        CacheWarm::Busy {
            real: true,
            last_real_end: None,
            warm: None,
        },
        CacheWarm::Busy {
            real: false,
            last_real_end: Some(at(0)),
            warm: Some(input(1)),
        },
        CacheWarm::Armed {
            last_real_end: at(0),
            fires_at: at(240),
        },
        CacheWarm::Warming {
            input: input(1),
            last_real_end: at(0),
        },
    ];
    let events = vec![
        CacheWarmEvent::TurnStarted {
            input: Some(input(1)),
        },
        CacheWarmEvent::TurnEnded {
            at: at(10),
            cadence: CADENCE,
        },
        CacheWarmEvent::TurnEnded {
            at: at(9000),
            cadence: CADENCE,
        },
        CacheWarmEvent::TimerFired {
            input: input(2),
            at: at(240),
            cadence: CADENCE,
        },
        CacheWarmEvent::TimerFired {
            input: input(3),
            at: at(9000),
            cadence: CADENCE,
        },
        CacheWarmEvent::InputJoined { input: input(4) },
        CacheWarmEvent::WarmRefused,
        CacheWarmEvent::SessionGone,
    ];
    let cases = states.into_iter().flat_map(|state| {
        events
            .clone()
            .into_iter()
            .map(move |event| (state.clone(), event))
    });

    // Act
    let checked = check_state_machine_transitions::<CacheWarmMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

fn step(state: CacheWarm, event: CacheWarmEvent) -> (CacheWarm, Vec<CacheWarmEffect>) {
    CacheWarmMachine::transition(&state, event).unwrap_or((state, Vec::new()))
}

/// Oracle: today's provider_cache_warm.rs behaviour (R16). A real turn's end
/// arms one warm prompt; the warm turn's own end re-arms without extending
/// the window; a refusal waits for the next turn end.
#[test]
fn a_real_turn_arms_a_warm_prompt_and_its_own_turns_never_extend_the_window() {
    // Arrange
    let cold = CacheWarm::Cold {
        last_real_end: None,
    };

    // Act
    let (busy, _) = step(cold, CacheWarmEvent::TurnStarted { input: None });
    let (armed, arm) = step(
        busy,
        CacheWarmEvent::TurnEnded {
            at: at(100),
            cadence: CADENCE,
        },
    );
    let (warming, submit) = step(
        armed,
        CacheWarmEvent::TimerFired {
            input: input(7),
            at: at(340),
            cadence: CADENCE,
        },
    );
    let (warm_turn, _) = step(
        warming,
        CacheWarmEvent::TurnStarted {
            input: Some(input(7)),
        },
    );
    let (rearmed, _) = step(
        warm_turn,
        CacheWarmEvent::TurnEnded {
            at: at(400),
            cadence: CADENCE,
        },
    );
    let (refused_from, _) = step(
        rearmed.clone(),
        CacheWarmEvent::TimerFired {
            input: input(8),
            at: at(640),
            cadence: CADENCE,
        },
    );
    let (after_refusal, _) = step(refused_from, CacheWarmEvent::WarmRefused);

    // Assert
    assert_eq!(arm, vec![CacheWarmEffect::ArmTimer { at: at(340) }]);
    assert_eq!(
        submit,
        vec![CacheWarmEffect::SubmitWarm { input: input(7) }]
    );
    assert_eq!(
        rearmed,
        CacheWarm::Armed {
            last_real_end: at(100),
            fires_at: at(640)
        }
    );
    assert_eq!(
        after_refusal,
        CacheWarm::Cold {
            last_real_end: Some(at(100))
        }
    );
}

/// Oracle: R16 over any order. A warm prompt is only submitted from Armed; an
/// arm is only made while the last real turn is inside the window; the
/// module's own warm turns never move the last real turn's end.
#[hegel::test]
fn any_event_order_keeps_the_warm_window(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = CacheWarm::Cold {
        last_real_end: None,
    };
    let mut clock: u64 = 0;
    let mut minted: u128 = 0;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(60)) {
        clock += u64::from(case.draw(gs::integers::<u16>().max_value(2000)));
        let event = match case.draw(gs::integers::<u8>().max_value(5)) {
            0 => {
                let own = match &state {
                    CacheWarm::Warming { input, .. } if case.draw(gs::booleans()) => {
                        Some(input.clone())
                    }
                    CacheWarm::Cold { .. }
                    | CacheWarm::Busy { .. }
                    | CacheWarm::Armed { .. }
                    | CacheWarm::Warming { .. } => None,
                };
                CacheWarmEvent::TurnStarted { input: own }
            }
            1 => CacheWarmEvent::TurnEnded {
                at: at(clock),
                cadence: CADENCE,
            },
            2 => {
                minted += 1;
                CacheWarmEvent::TimerFired {
                    input: input(minted),
                    at: at(clock),
                    cadence: CADENCE,
                }
            }
            3 => CacheWarmEvent::WarmRefused,
            4 => {
                minted += 1;
                CacheWarmEvent::InputJoined {
                    input: input(minted),
                }
            }
            _ => CacheWarmEvent::SessionGone,
        };
        let before = state.clone();
        let Ok((next, effects)) = CacheWarmMachine::transition(&state, event.clone()) else {
            continue;
        };
        for effect in &effects {
            match effect {
                CacheWarmEffect::SubmitWarm { .. } => {
                    let CacheWarm::Armed { last_real_end, .. } = &before else {
                        panic!("submitted from {before:?}");
                    };
                    assert!(
                        at(clock).saturating_duration_since(*last_real_end) < CADENCE.keep_warm_for,
                        "submitted after the window"
                    );
                }
                CacheWarmEffect::ArmTimer { .. } => {
                    let CacheWarm::Armed { last_real_end, .. } = &next else {
                        panic!("armed without the Armed state");
                    };
                    assert!(
                        at(clock).saturating_duration_since(*last_real_end) < CADENCE.keep_warm_for
                    );
                }
                CacheWarmEffect::Disarm => {}
            }
        }
        if let (
            CacheWarm::Busy {
                real: false,
                last_real_end: was,
                ..
            },
            CacheWarmEvent::TurnEnded { .. },
        ) = (&before, &event)
        {
            assert_eq!(last_real(&next), *was, "an own warm turn moved the window");
        }
        state = next;
    }
}

/// Oracle: design note §3.7 ("Armed | TimerFired (window over) → Cold") and
/// today's begin_warm_turn: a timer that fires after the last real turn's
/// window warms nothing and leaves the session cold.
#[test]
fn a_timer_firing_after_the_window_warms_nothing() {
    // Arrange
    let armed = CacheWarm::Armed {
        last_real_end: at(0),
        fires_at: at(3590),
    };

    // Act
    let (after, effects) = step(
        armed,
        CacheWarmEvent::TimerFired {
            input: input(1),
            at: at(3700),
            cadence: CADENCE,
        },
    );

    // Assert
    assert!(effects.is_empty(), "no warm prompt: {effects:?}");
    assert_eq!(
        after,
        CacheWarm::Cold {
            last_real_end: Some(at(0))
        }
    );
}

/// Oracle: today's mark_real_unless_warm (R16): an input that joins a warm
/// turn (a steer) makes it real work, so its end moves the window.
#[test]
fn an_input_joining_a_warm_turn_makes_it_real() {
    // Arrange
    let warming = CacheWarm::Warming {
        input: input(7),
        last_real_end: at(100),
    };
    let (warm_turn, _) = step(
        warming,
        CacheWarmEvent::TurnStarted {
            input: Some(input(7)),
        },
    );

    // Act
    let (joined, _) = step(warm_turn, CacheWarmEvent::InputJoined { input: input(8) });
    let (after, _) = step(
        joined,
        CacheWarmEvent::TurnEnded {
            at: at(400),
            cadence: CADENCE,
        },
    );

    // Assert
    assert_eq!(
        after,
        CacheWarm::Armed {
            last_real_end: at(400),
            fires_at: at(640)
        }
    );
}
