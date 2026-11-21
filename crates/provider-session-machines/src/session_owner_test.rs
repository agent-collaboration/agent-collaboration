use super::*;
use uuid::Uuid;

#[path = "session_owner/owner_ports_test.rs"]
mod owner_contract;
#[path = "session_owner/owner_followup_test.rs"]
mod owner_followup;
#[path = "session_owner/owner_history_test.rs"]
mod owner_history;
#[path = "session_owner/owner_property_test.rs"]
mod owner_property;

fn uuid(number: u64) -> Uuid {
    // Deterministic UUIDv7 inputs keep clock reads out of the property.
    Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | u128::from(number))
}

fn fresh() -> SessionOwnerState {
    SessionOwnerState::new(OwnerAddress {
        incarnation: OwnerIncarnation(uuid(1000)),
        connection_epoch: ConnectionEpoch(uuid(1001)),
    })
}

fn input(generation: SessionGeneration, number: u64) -> OwnerInput {
    OwnerInput {
        generation,
        operation: uuid(20_000 + number),
        input: InputId::new(uuid(number).to_string()).expect("fixture input identity"),
        turn: TurnId(uuid(10_000 + number)),
    }
}

fn decide(state: &SessionOwnerState, event: OwnerEvent) -> SessionOwnerDecision {
    state.decide(OwnerMailboxEvent {
        owner: state.snapshot().owner,
        event,
    })
}

fn apply(state: &mut SessionOwnerState, event: OwnerEvent) -> Vec<OwnerEffect> {
    let mut effects = apply_pending(state, event);
    let writes: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, .. }) => Some(*write),
            _ => None,
        })
        .collect();
    for write in writes {
        effects.extend(apply_pending(
            state,
            OwnerEvent::Store(SessionStoreResult::Terminal {
                write,
                outcome: EffectOutcome::Applied,
            }),
        ));
    }
    effects
}

fn apply_pending(state: &mut SessionOwnerState, event: OwnerEvent) -> Vec<OwnerEffect> {
    let decision = decide(state, event).expect("fixture event is current and permitted");
    *state = decision.next;
    decision.effects.effects
}

fn command(state: &mut SessionOwnerState, command: SessionOwnerCommand) -> Vec<OwnerEffect> {
    apply(state, OwnerEvent::Command(command))
}

fn deadline(state: &mut SessionOwnerState, deadline: OwnerDeadline) -> Vec<OwnerEffect> {
    apply(state, OwnerEvent::Deadline(deadline))
}

fn assert_late(state: &SessionOwnerState, event: OwnerEvent) {
    assert!(matches!(
        decide(state, event),
        Err(SessionOwnerRejection::IgnoredLate)
    ));
}

fn start_restore(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    command(
        state,
        SessionOwnerCommand::Restore {
            attempt: LoadAttempt(uuid(number)),
            via: RestoreVia::Load,
        },
    )
    .into_iter()
    .find_map(|effect| match effect {
        OwnerEffect::Store(SessionStoreRequest::ReadRestoreSetup(run)) => Some(run),
        _ => None,
    })
    .expect("owner released setup read")
}

fn read_setup(state: &mut SessionOwnerState, run: RunAttempt<LoadAttempt>) -> Vec<OwnerEffect> {
    apply(
        state,
        OwnerEvent::Store(SessionStoreResult::RestoreSetup {
            run,
            outcome: EffectOutcome::Applied,
        }),
    )
}

fn open_wire(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    let run = start_restore(state, number);
    assert!(
        read_setup(state, run).contains(&OwnerEffect::Wire(SessionWireRequest::Restore {
            run,
            via: RestoreVia::Load
        }))
    );
    run
}

fn restore_reply(run: RunAttempt<LoadAttempt>) -> OwnerEvent {
    OwnerEvent::Wire(SessionWireResult::Restore {
        run,
        outcome: WireOutcome::Confirmed(RestoreLoadReadback {
            catalogue: Arc::default(),
            router_auto_requested: false,
            mode: RestoredMode::Observed,
            fast_on: None,
        }),
    })
}

fn host_ready(run: RunAttempt<LoadAttempt>) -> OwnerEvent {
    OwnerEvent::Prepare(RestorePreparation::HostReady {
        run,
        capabilities: session_event_model::CapabilityReport::default(),
        settings: session_event_model::SessionSettings::default(),
        observed: BTreeMap::new(),
    })
}

fn finish_preparation(
    state: &mut SessionOwnerState,
    run: RunAttempt<LoadAttempt>,
) -> Vec<OwnerEffect> {
    apply(
        state,
        OwnerEvent::Prepare(RestorePreparation::RouteReady(run)),
    );
    apply(state, host_ready(run))
}

fn restore_live(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    let run = open_wire(state, number);
    apply(state, restore_reply(run));
    finish_preparation(state, run);
    run
}

fn start_close(
    state: &mut SessionOwnerState,
    generation: SessionGeneration,
    number: u64,
) -> RunAttempt<CloseAttempt> {
    let attempt = CloseAttempt(uuid(number));
    command(
        state,
        SessionOwnerCommand::Close {
            generation,
            attempt,
        },
    );
    RunAttempt {
        generation,
        attempt,
    }
}

fn close_reply(run: RunAttempt<CloseAttempt>, outcome: WireOutcome<()>) -> OwnerEvent {
    OwnerEvent::Wire(SessionWireResult::Close { run, outcome })
}

fn mark_prompt(input: &OwnerInput) -> OwnerEvent {
    OwnerEvent::Store(SessionStoreResult::Dispatch {
        input: input.clone(),
        outcome: EffectOutcome::Applied,
    })
}

fn enqueue_prompt(state: &mut SessionOwnerState, input: &OwnerInput) -> Vec<OwnerEffect> {
    command(
        state,
        SessionOwnerCommand::Prompt {
            input: input.clone(),
            admission: PromptAdmission::Auto {
                steering_supported: true,
            },
        },
    )
}

fn authorization(effects: &[OwnerEffect]) -> DispatchAuthorization {
    effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Authorize { authorization, .. }
            | OwnerEffect::DrainCancel(authorization) => Some(authorization.clone()),
            _ => None,
        })
        .expect("authorization")
}

fn send_prompt(state: &mut SessionOwnerState, input: &OwnerInput) -> DispatchAuthorization {
    enqueue_prompt(state, input);
    let mark = apply(state, mark_prompt(input));
    let key = authorization(&mark);
    assert!(mark.contains(&OwnerEffect::Authorize {
        authorization: key.clone(),
        work: AuthorizedWork::Prompt(input.clone())
    }));
    apply(
        state,
        OwnerEvent::Dispatch(DispatchResult::Taken(key.clone())),
    );
    key
}

fn end_turn(
    state: &mut SessionOwnerState,
    authorization: DispatchAuthorization,
) -> Vec<OwnerEffect> {
    apply(
        state,
        OwnerEvent::Dispatch(DispatchResult::PromptFinished {
            authorization,
            outcome: WireOutcome::Confirmed(PromptCompletion {
                stop_reason: session_event_model::StopReason::EndTurn,
                local_cause: None,
                response: String::new(),
            }),
        }),
    )
}

#[test]
fn unknown_prompt_offers_terminal_then_retires_the_line_known_unsent() {
    let mut state = fresh();
    let run = restore_live(&mut state, 10);
    let foreground = input(run.generation, 11);
    let authorization = send_prompt(&mut state, &foreground);
    let queued = [input(run.generation, 12), input(run.generation, 13)];
    for pending in &queued {
        command(
            &mut state,
            SessionOwnerCommand::Prompt {
                input: pending.clone(),
                admission: PromptAdmission::Queue,
            },
        );
    }

    let effects = apply_pending(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::PromptFinished {
            authorization,
            outcome: WireOutcome::Unknown,
        }),
    );

    let replies: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { reply, .. }) => {
                Some(reply.as_ref().clone())
            }
            _ => None,
        })
        .collect();
    let blocking_operation = foreground.operation;
    let mut expected = vec![SessionOwnerReply::InputFinished {
        input: foreground,
        remote: RemoteEffect::Unknown,
    }];
    expected.extend(queued.map(|input| SessionOwnerReply::InputUnsent {
        input,
        reason: RevokeReason::EarlierOutcomeUnknown {
            operation: blocking_operation,
        },
    }));
    assert_eq!(
        replies, expected,
        "terminal offers must preserve the uncertainty order"
    );
    assert!(state.snapshot().queue.is_empty());
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Store(SessionStoreRequest::MarkDispatch(_)) | OwnerEffect::Authorize { .. }
    )));
}

#[test]
fn selected_skill_disappearing_while_queued_or_marking_is_never_sent() {
    use session_event_model::{
        ProviderSkill, ProviderSkillScope, SelectedSkill, SkillInvocationName, SkillView,
    };

    for marking in [false, true] {
        let mut state = fresh();
        let run = restore_live(&mut state, 1);
        let skill = SkillInvocationName::new("review").expect("skill identity");
        apply(
            &mut state,
            OwnerEvent::SkillView {
                run: run.generation,
                view: SkillView::Available {
                    skills: vec![ProviderSkill {
                        invocation_name: skill.clone(),
                        description: "Review".to_owned(),
                        scope: ProviderSkillScope::Repo,
                    }],
                    errors: Vec::new(),
                },
            },
        );
        let foreground = (!marking).then(|| send_prompt(&mut state, &input(run.generation, 2)));
        let queued = input(run.generation, 3);
        let admitted = command(
            &mut state,
            SessionOwnerCommand::PromptWithSkills {
                input: queued.clone(),
                admission: if marking {
                    PromptAdmission::Direct
                } else {
                    PromptAdmission::Queue
                },
                selected: vec![SelectedSkill {
                    invocation_name: skill,
                }],
            },
        );
        assert_eq!(
            admitted.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
                queued.clone()
            ))),
            marking
        );
        apply(
            &mut state,
            OwnerEvent::SkillView {
                run: run.generation,
                view: SkillView::Available {
                    skills: Vec::new(),
                    errors: Vec::new(),
                },
            },
        );
        let mut after_removal = if let Some(foreground) = foreground {
            end_turn(&mut state, foreground)
        } else {
            apply(&mut state, mark_prompt(&queued))
        };
        assert!(!after_removal.iter().any(|effect| matches!(
            effect,
            OwnerEffect::Authorize { .. }
                | OwnerEffect::Store(SessionStoreRequest::MarkDispatch(_))
        )));
        if marking {
            assert!(after_removal.contains(&OwnerEffect::Store(
                SessionStoreRequest::UndoDispatch(queued.clone())
            )));
            after_removal.extend(apply(
                &mut state,
                OwnerEvent::Store(SessionStoreResult::UndoDispatch {
                    input: queued.clone(),
                    outcome: EffectOutcome::Applied,
                }),
            ));
        }
        assert!(
            after_removal.contains(&OwnerEffect::Reply(SessionOwnerReply::InputUnsent {
                input: queued,
                reason: RevokeReason::SkillSelectionChanged,
            }))
        );
    }
}

#[test]
fn connection_loss_leaves_settled_turn_evidence_and_finishes_only_the_running_turn() {
    let mut settled = fresh();
    let settled_run = restore_live(&mut settled, 10);
    let first = input(settled_run.generation, 11);
    let authorization = send_prompt(&mut settled, &first);
    end_turn(&mut settled, authorization);
    let ended = settled.snapshot().last_live_turn.expect("known terminal");

    let mut running = fresh();
    let running_run = restore_live(&mut running, 20);
    let second = input(running_run.generation, 21);
    send_prompt(&mut running, &second);

    let settled_effects = apply(&mut settled, OwnerEvent::ConnectionLost);
    let running_effects = apply(&mut running, OwnerEvent::ConnectionLost);

    assert_eq!(settled.snapshot().last_live_turn, Some(ended));
    assert!(!settled_effects.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Reply(SessionOwnerReply::InputFinished { .. })
    )));
    assert!(
        running_effects.contains(&OwnerEffect::Reply(SessionOwnerReply::InputFinished {
            input: second,
            remote: RemoteEffect::Unknown,
        }))
    );
    assert_late(&running, OwnerEvent::ConnectionLost);
    assert_late(&settled, OwnerEvent::ConnectionLost);
}

fn publication_for(generation: SessionGeneration, effects: &[OwnerEffect]) -> Option<BatchId> {
    effects.iter().find_map(|effect| match effect {
        OwnerEffect::Publish(PublicationBatch::Commit { id, facts }) if facts.iter().any(|fact| matches!(fact,
            PublicationFact::Live(SessionPublication::Activated { run, .. }) if run.generation == generation)) => Some(*id),
        _ => None,
    })
}

fn tool_forwarded(effects: &[OwnerEffect], generation: SessionGeneration) -> bool {
    effects.iter().any(|effect| matches!(effect, OwnerEffect::ForwardTool { run, tool_id, .. } if *run == generation && tool_id == "provider-tool-id"))
}

fn tool_update(generation: SessionGeneration, observation: ToolObservation) -> OwnerEvent {
    OwnerEvent::RoutedTool(RoutedTool::Observed {
        run: generation,
        turn: None,
        tool_id: "provider-tool-id".to_owned(),
        observation,
    })
}

fn callback(generation: SessionGeneration, turn: TurnId) -> OwnerEvent {
    OwnerEvent::RoutedTool(RoutedTool::Exact {
        run: generation,
        turn,
        tool_id: "provider-tool-id".to_owned(),
    })
}

fn setting_command(setting: OwnerSetting, number: u64) -> SessionOwnerCommand {
    SessionOwnerCommand::Set {
        input: input(setting.change.generation, number).input,
        setting,
        value: SettingValueId("off".to_owned()),
        expected_model: None,
    }
}

fn setting_authorized(
    state: &mut SessionOwnerState,
    setting: OwnerSetting,
    number: u64,
) -> DispatchAuthorization {
    let key = authorization(&command(state, setting_command(setting, number)));
    apply(
        state,
        OwnerEvent::Dispatch(DispatchResult::Taken(key.clone())),
    );
    key
}
