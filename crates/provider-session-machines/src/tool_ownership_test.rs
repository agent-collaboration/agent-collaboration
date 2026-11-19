use super::{
    ToolObservation, ToolOrigin, ToolOwnership, ToolOwnershipEffect, ToolOwnershipEvent,
    ToolSessionPhase, reduce_tool_ownership,
};
use crate::{SessionGeneration, session_turn_machine::TurnId};
use hegel::generators as gs;
use std::collections::HashMap;

fn generation(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

fn finish(records: &mut HashMap<u8, ToolOwnership>, event: ToolOwnershipEvent) {
    for record in records.values_mut() {
        let (next, effect) = reduce_tool_ownership(Some(*record), event);
        assert_eq!(effect, ToolOwnershipEffect::None);
        assert!(next.is_some(), "retirement discarded an observed ID");
        if let Some(next) = next {
            *record = next;
        }
    }
}

#[hegel::test]
fn tool_frames_and_callbacks_stay_with_the_generation_that_observed_them(case: hegel::TestCase) {
    let mut records = HashMap::new();
    let mut observations = HashMap::new();
    let mut run = 0_u64;
    let mut turn_sequence = 0_u128;
    let mut phase = None;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(80)) {
        let operation = case.draw(gs::integers::<u8>().max_value(9));
        match operation {
            0 | 1 => {
                // Interleave newer starts with any late notifications from a
                // prior run, including a delayed retirement of its owners.
                run += 1;
                phase = Some(if operation == 0 {
                    ToolSessionPhase::Loading {
                        generation: generation(run),
                    }
                } else {
                    ToolSessionPhase::LiveIdle {
                        generation: generation(run),
                    }
                });
            }
            2 if matches!(phase, Some(ToolSessionPhase::Loading { .. })) => {
                phase = Some(ToolSessionPhase::LiveIdle {
                    generation: generation(run),
                });
            }
            3 if matches!(phase, Some(ToolSessionPhase::LiveIdle { .. })) => {
                turn_sequence += 1;
                let turn = TurnId(uuid::Uuid::from_u128(
                    0x0000_0000_0000_7000_8000_0000_0000_0000 | turn_sequence,
                ));
                phase = Some(ToolSessionPhase::LiveRunning {
                    generation: generation(run),
                    turn,
                });
            }
            4 => {
                if let Some(ToolSessionPhase::LiveRunning { generation, turn }) = phase {
                    finish(
                        &mut records,
                        ToolOwnershipEvent::TurnEnded { generation, turn },
                    );
                    phase = Some(ToolSessionPhase::LiveIdle { generation });
                }
            }
            5 | 6 if run > 0 => {
                let retired = generation(if operation == 5 { run } else { run - 1 });
                finish(
                    &mut records,
                    ToolOwnershipEvent::RunRetired {
                        generation: retired,
                    },
                );
                if operation == 5 {
                    phase = None;
                }
            }
            7..=9 => {
                let Some(phase) = phase else { continue };
                let id = case.draw(gs::integers::<u8>().max_value(3));
                let previous = records.get(&id).copied();
                let event = match operation {
                    7 => ToolOwnershipEvent::Observe {
                        phase,
                        observation: ToolObservation::Announcement,
                    },
                    8 => ToolOwnershipEvent::Observe {
                        phase,
                        observation: ToolObservation::Continuation,
                    },
                    _ => ToolOwnershipEvent::Callback { phase },
                };
                let (next, effect) = reduce_tool_ownership(previous, event);
                if matches!(previous, Some(ToolOwnership::Retired { .. }))
                    && !matches!(phase, ToolSessionPhase::Loading { .. })
                {
                    assert!(
                        matches!(
                            effect,
                            ToolOwnershipEffect::None | ToolOwnershipEffect::CheckRetainedItem
                        ),
                        "a live frame or callback accepted a retired ID before projection"
                    );
                }
                match effect {
                    ToolOwnershipEffect::ForwardFrame {
                        generation: published,
                    } => {
                        assert!(matches!(event, ToolOwnershipEvent::Observe { .. }));
                        assert_eq!(published, generation(run));
                        assert_eq!(
                            next.and_then(ToolOwnership::owned_generation),
                            Some(published)
                        );
                        observations.insert(id, published);
                    }
                    ToolOwnershipEffect::ResolveOwner {
                        generation: published,
                    } => {
                        assert!(matches!(event, ToolOwnershipEvent::Callback { .. }));
                        assert_eq!(
                            published,
                            generation(run),
                            "callback used an older generation"
                        );
                        assert_eq!(
                            observations.get(&id),
                            Some(&published),
                            "callback's generation never observed the tool"
                        );
                    }
                    ToolOwnershipEffect::UseSoleRunningTurn {
                        generation: published,
                    } => {
                        assert!(matches!(phase, ToolSessionPhase::LiveRunning { .. }));
                        assert_eq!(published, generation(run));
                        assert!(
                            !observations.contains_key(&id),
                            "observed ID reached the unknown fallback"
                        );
                    }
                    ToolOwnershipEffect::CheckRetainedItem => assert!(matches!(
                        event,
                        ToolOwnershipEvent::Observe {
                            phase: ToolSessionPhase::LiveIdle { generation },
                            observation: ToolObservation::Continuation,
                        } if matches!(previous, Some(ToolOwnership::Retired { generation: prior }) if prior == generation)
                    )),
                    ToolOwnershipEffect::ContinueRetainedItem { .. } => {
                        panic!("a frame published before consulting the projection")
                    }
                    ToolOwnershipEffect::None => {}
                }
                if let Some(next) = next {
                    records.insert(id, next);
                } else {
                    assert!(!observations.contains_key(&id), "tracked ID became unknown");
                }
            }
            _ => {}
        }
    }
}

#[test]
fn loading_refreshes_an_older_tombstone_but_never_reopens_a_retired_generation() {
    let old = ToolOwnership::Retired {
        generation: generation(1),
    };
    let same = ToolOwnershipEvent::Observe {
        phase: ToolSessionPhase::Loading {
            generation: generation(1),
        },
        observation: ToolObservation::Continuation,
    };
    assert_eq!(
        reduce_tool_ownership(Some(old), same),
        (Some(old), ToolOwnershipEffect::None)
    );

    let new = ToolOwnershipEvent::Observe {
        phase: ToolSessionPhase::Loading {
            generation: generation(3),
        },
        observation: ToolObservation::Announcement,
    };
    assert_eq!(
        reduce_tool_ownership(Some(old), new),
        (
            Some(ToolOwnership::Owned {
                generation: generation(3),
                origin: ToolOrigin::Replay,
                turn: None
            }),
            ToolOwnershipEffect::ForwardFrame {
                generation: generation(3)
            },
        ),
    );
}

#[test]
fn an_owned_id_cannot_publish_in_another_generation() {
    let old = ToolOwnership::Owned {
        generation: generation(1),
        origin: ToolOrigin::Replay,
        turn: None,
    };
    let phase = ToolSessionPhase::LiveIdle {
        generation: generation(2),
    };
    for event in [
        ToolOwnershipEvent::Observe {
            phase,
            observation: ToolObservation::Continuation,
        },
        ToolOwnershipEvent::Callback { phase },
    ] {
        assert_eq!(
            reduce_tool_ownership(Some(old), event),
            (Some(old), ToolOwnershipEffect::None),
            "a second generation used the first generation's tool"
        );
    }
}

#[hegel::test]
fn only_same_generation_idle_completion_of_an_existing_item_publishes(case: hegel::TestCase) {
    let prior = u64::from(case.draw(gs::integers::<u8>()));
    let newer = prior + u64::from(case.draw(gs::integers::<u8>().min_value(1)));
    let owner = Some(ToolOwnership::Retired {
        generation: generation(prior),
    });
    let turn = TurnId(uuid::Uuid::from_u128(
        0x0000_0000_0000_7000_8000_0000_0000_0001,
    ));
    for current in [generation(prior), generation(newer)] {
        let idle = ToolSessionPhase::LiveIdle {
            generation: current,
        };
        let running = ToolSessionPhase::LiveRunning {
            generation: current,
            turn,
        };
        for phase in [idle, running] {
            for item_exists in [true, false] {
                let (after, effect) = reduce_tool_ownership(
                    owner,
                    ToolOwnershipEvent::ProjectRetiredUpdate { phase, item_exists },
                );
                assert_eq!(after, owner);
                if current == generation(prior) && phase == idle && item_exists {
                    assert_eq!(
                        effect,
                        ToolOwnershipEffect::ContinueRetainedItem {
                            generation: current
                        },
                        "same-generation idle completion must publish"
                    );
                } else {
                    assert_eq!(
                        effect,
                        ToolOwnershipEffect::None,
                        "an older, running, or absent item must never publish"
                    );
                }
            }
            for event in [
                ToolOwnershipEvent::Observe {
                    phase,
                    observation: ToolObservation::Announcement,
                },
                ToolOwnershipEvent::Callback { phase },
            ] {
                assert_eq!(
                    reduce_tool_ownership(owner, event),
                    (owner, ToolOwnershipEffect::None)
                );
            }
        }
    }
}
