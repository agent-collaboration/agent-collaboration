use super::*;

fn submit_evidenced_queue(state: &mut SessionOwnerState, input: OwnerInput) -> Vec<OwnerEffect> {
    command(
        state,
        SessionOwnerCommand::PlaceDelivery {
            input,
            delivery: DeliveryInput::Prompt {
                admission: PromptAdmission::Queue,
                selected: Vec::new(),
            },
        },
    )
}

fn pending_placement(effects: &[OwnerEffect], kind: DeliveryPlacementKind) -> DeliveryPlacement {
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Store(
            SessionStoreRequest::MarkDispatch(_)
                | SessionStoreRequest::MarkQueuedDispatch(_)
                | SessionStoreRequest::MarkSteerDispatch(_)
        ) | OwnerEffect::Authorize { .. }
    )));
    effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::RecordDeliveryEvidence(placement) if placement.kind == kind => {
                Some(placement.clone())
            }
            _ => None,
        })
        .expect("one placement offered before its mark")
}

#[test]
fn t4_cold_queue_records_intake_before_releasing_the_exact_restore() {
    let mut state = fresh();
    let queued = input(SessionGeneration::from_number(1), 2);
    let attempt = LoadAttempt(uuid(3));
    let held = command(
        &mut state,
        SessionOwnerCommand::QueueColdDelivery {
            input: queued.clone(),
            attempt,
            selected: Vec::new(),
            record_evidence: true,
        },
    );
    let intake = pending_placement(&held, DeliveryPlacementKind::Queue);
    assert!(!held.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Store(SessionStoreRequest::ReadRestoreSetup(_))
    )));

    let released = apply(
        &mut state,
        OwnerEvent::DeliveryEvidence {
            placement: intake,
            outcome: EffectOutcome::Applied,
        },
    );
    let accepted = released
        .iter()
        .position(|effect| *effect == OwnerEffect::InputQueued(queued.clone()))
        .expect("queue acceptance before the restore can settle");
    let setup_read = released
        .iter()
        .position(|effect| {
            *effect
                == OwnerEffect::Store(SessionStoreRequest::ReadRestoreSetup(RunAttempt {
                    generation: queued.generation,
                    attempt,
                }))
        })
        .expect("the queued input releases its exact restore's setup read");
    assert!(accepted < setup_read);
}

#[test]
fn t2_queued_delivery_records_intake_then_dispatch_evidence_before_its_first_mark() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let queued = input(run.generation, 2);
    let admission = submit_evidenced_queue(&mut state, queued.clone());
    let intake = pending_placement(&admission, DeliveryPlacementKind::Queue);
    assert_eq!(intake.input, queued);
    assert!(!admission.contains(&OwnerEffect::InputQueued(queued.clone())));

    let dispatching = apply(
        &mut state,
        OwnerEvent::DeliveryEvidence {
            placement: intake,
            outcome: EffectOutcome::Applied,
        },
    );
    assert!(dispatching.contains(&OwnerEffect::InputQueued(queued.clone())));
    let dispatch = pending_placement(&dispatching, DeliveryPlacementKind::Start);
    assert_eq!(dispatch.input, queued);
    let marked = apply(
        &mut state,
        OwnerEvent::DeliveryEvidence {
            placement: dispatch,
            outcome: EffectOutcome::Applied,
        },
    );
    assert!(marked.contains(&OwnerEffect::Store(
        SessionStoreRequest::MarkQueuedDispatch(queued)
    )));
}

#[test]
fn t2_expired_dispatch_evidence_retires_a_queue_input_before_its_first_mark() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let queued = input(run.generation, 2);
    let admission = submit_evidenced_queue(&mut state, queued.clone());
    let intake = pending_placement(&admission, DeliveryPlacementKind::Queue);
    let dispatching = apply(
        &mut state,
        OwnerEvent::DeliveryEvidence {
            placement: intake,
            outcome: EffectOutcome::Applied,
        },
    );
    let dispatch = pending_placement(&dispatching, DeliveryPlacementKind::Start);
    let effects = apply(
        &mut state,
        OwnerEvent::Deadline(OwnerDeadline::DeliveryEvidence(dispatch.clone())),
    );
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Store(
            SessionStoreRequest::MarkDispatch(_) | SessionStoreRequest::MarkQueuedDispatch(_)
        ) | OwnerEffect::Authorize { .. }
    )));
    assert!(effects.iter().any(|effect| matches!(effect,
        OwnerEffect::Store(SessionStoreRequest::RecordTerminal { reply, .. })
            if **reply == SessionOwnerReply::InputUnsent { input: queued.clone(), reason: RevokeReason::DeliveryEvidenceUnavailable }
    )));
    let late = state.decide(OwnerMailboxEvent {
        owner: dispatch.owner,
        event: OwnerEvent::DeliveryEvidence {
            placement: dispatch,
            outcome: EffectOutcome::Applied,
        },
    });
    assert!(matches!(late, Err(SessionOwnerRejection::IgnoredLate)));
}

fn pending_steer(state: &mut SessionOwnerState, foreground: &OwnerInput) -> OwnerInput {
    let request = OwnerInput {
        turn: foreground.turn,
        ..input(foreground.generation, 3)
    };
    command(
        state,
        SessionOwnerCommand::Steer {
            input: request.clone(),
            supported: true,
        },
    );
    request
}

fn steer_marked(request: &OwnerInput) -> OwnerEvent {
    OwnerEvent::Store(SessionStoreResult::SteerDispatch {
        input: request.clone(),
        outcome: EffectOutcome::Applied,
    })
}

fn no_membership_change(effects: &[OwnerEffect]) {
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, OwnerEffect::CallbackMembership(_)))
    );
}

#[test]
fn t2_existing_turn_take_rejects_a_target_when_b_replaces_a_on_the_same_run() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let first = input(run.generation, 2);
    let first_key = send_prompt(&mut state, &first);
    assert_eq!(first_key.target, AuthorizationTarget::NewTurn);
    let request = pending_steer(&mut state, &first);
    let effects = apply(&mut state, steer_marked(&request));
    let steer = authorization(&effects);
    assert_eq!(
        steer.target,
        AuthorizationTarget::ExistingTurn { turn: first.turn }
    );
    assert!(steer.matches_route(run.generation, Some(first.turn)));
    no_membership_change(&effects);

    // A successful transport take can be reported after A has ended. Only
    // the atomic route check, not the owner at result time, decides the send.
    end_turn(&mut state, first_key);
    let replacement = input(run.generation, 4);
    send_prompt(&mut state, &replacement);
    let SessionTurn::Running { turn: current } = state.snapshot().turn else {
        panic!("replacement turn did not start");
    };
    assert_eq!(current, replacement.turn);
    assert!(
        !steer.matches_route(run.generation, Some(current)),
        "T2 a held take must reject A once B is the current route turn"
    );
    assert!(!steer.matches_route(run.generation, None));

    let rejected = apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::StaleTarget(steer.clone())),
    );
    no_membership_change(&rejected);
    assert!(
        rejected.contains(&OwnerEffect::Store(SessionStoreRequest::UndoDispatch(
            request.clone()
        )))
    );
    let settled = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::UndoDispatch {
            input: request.clone(),
            outcome: EffectOutcome::Applied,
        }),
    );
    assert!(
        settled.contains(&OwnerEffect::Reply(SessionOwnerReply::InputUnsent {
            input: request,
            reason: RevokeReason::TurnEnded,
        }))
    );
    assert_eq!(
        RevokeReason::TurnEnded.human_phrase(),
        "the turn you steered had already ended; send again"
    );
    assert_eq!(
        state.snapshot().turn,
        SessionTurn::Running {
            turn: replacement.turn
        }
    );
    assert_late(&state, OwnerEvent::Dispatch(DispatchResult::Taken(steer)));
}

#[test]
fn t2_revoking_a_cancel_does_not_change_its_existing_turn_membership() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let foreground = input(run.generation, 2);
    send_prompt(&mut state, &foreground);
    let effects = command(
        &mut state,
        SessionOwnerCommand::CancelTurn {
            generation: run.generation,
            turn: foreground.turn,
        },
    );
    let cancel = authorization(&effects);
    assert_eq!(
        cancel.target,
        AuthorizationTarget::ExistingTurn {
            turn: foreground.turn
        }
    );
    assert!(cancel.matches_route(run.generation, Some(foreground.turn)));
    no_membership_change(&effects);

    let closed = command(
        &mut state,
        SessionOwnerCommand::Close {
            generation: run.generation,
            attempt: CloseAttempt(uuid(3)),
        },
    );
    assert!(closed.contains(&OwnerEffect::Revoke {
        authorization: cancel.clone(),
        reason: RevokeReason::Closed,
    }));
    let revoked = apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Revoked(cancel.clone())),
    );
    no_membership_change(&revoked);
    assert!(revoked.contains(&OwnerEffect::CancelNotSent {
        authorization: cancel,
        operation: foreground.operation,
        reason: RevokeReason::Closed,
    }));
    assert_eq!(
        state.snapshot().turn,
        SessionTurn::Cancelling {
            turn: foreground.turn
        }
    );
}

#[test]
fn t8_late_dispatch_mark_has_a_known_unsent_terminal_offer_already_behind_it() {
    for steer in [false, true] {
        let mut state = fresh();
        let run = restore_live(&mut state, 1);
        let request = if steer {
            let foreground = input(run.generation, 2);
            send_prompt(&mut state, &foreground);
            pending_steer(&mut state, &foreground)
        } else {
            let request = input(run.generation, 2);
            let offered = enqueue_prompt(&mut state, &request);
            assert!(
                offered.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
                    request.clone()
                )))
            );
            request
        };
        let terminal = apply_pending(
            &mut state,
            OwnerEvent::Deadline(OwnerDeadline::DispatchMark {
                input: request.clone(),
            }),
        );
        let expected = SessionOwnerReply::InputFinished {
            input: request.clone(),
            remote: RemoteEffect::None,
        };
        let write = terminal
            .iter()
            .find_map(|effect| match effect {
                OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, reply })
                    if **reply == expected =>
                {
                    Some(*write)
                }
                _ => None,
            })
            .expect("known-unsent write must already be offered to the lane");
        assert!(
            !terminal
                .iter()
                .any(|effect| matches!(effect, OwnerEffect::Authorize { .. }))
        );
        assert!(terminal.contains(&OwnerEffect::Arm {
            deadline: OwnerDeadline::TerminalStore(write),
            budget: std::time::Duration::from_secs(10),
        }));

        // The lane can commit its older accepted mark after L. Only the
        // terminal offer remains active; the late mark cannot open a slot.
        let late = if steer {
            steer_marked(&request)
        } else {
            mark_prompt(&request)
        };
        assert_late(&state, late.clone());
        let next = SessionOwnerState::new(OwnerAddress {
            incarnation: OwnerIncarnation(uuid(50)),
            ..state.snapshot().owner
        });
        assert!(matches!(
            next.decide(OwnerMailboxEvent {
                owner: state.snapshot().owner,
                event: late
            }),
            Err(SessionOwnerRejection::IgnoredLate)
        ));
        let persisted = apply(
            &mut state,
            OwnerEvent::Store(SessionStoreResult::Terminal {
                write,
                outcome: EffectOutcome::Applied,
            }),
        );
        assert!(persisted.contains(&OwnerEffect::Reply(expected.clone())));
        assert_late(
            &state,
            OwnerEvent::Deadline(OwnerDeadline::TerminalStore(write)),
        );
    }
}

#[test]
fn t8_failed_known_unsent_cleanup_does_not_claim_that_the_mark_was_cleared() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let request = input(run.generation, 2);
    enqueue_prompt(&mut state, &request);
    let effects = apply_pending(
        &mut state,
        OwnerEvent::Deadline(OwnerDeadline::DispatchMark {
            input: request.clone(),
        }),
    );
    let write = effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, .. }) => Some(*write),
            _ => None,
        })
        .expect("known-unsent terminal offer");
    let failed = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::Terminal {
            write,
            outcome: EffectOutcome::Failed,
        }),
    );
    assert_eq!(
        failed,
        vec![OwnerEffect::ReplyUnsaved(
            SessionOwnerReply::InputFinished {
                input: request,
                remote: RemoteEffect::None,
            }
        )]
    );
}

fn assert_usable_with_mode_delta(
    state: &mut SessionOwnerState,
    run: RunAttempt<LoadAttempt>,
    current: &str,
    cause: ObservationCause,
) {
    let effects = finish_preparation(state, run);
    assert!(publication_for(run.generation, &effects).is_some());
    let result = state
        .snapshot()
        .last_restore
        .expect("immutable restore result");
    assert_eq!(result.attempt, run);
    assert!(matches!(
        result.disposition,
        RestoreDisposition::Restored { .. }
    ));
    assert_eq!(
        result.observed.get(&ProviderSettingKind::Mode),
        Some(&SettingEvidence {
            value: ObservedSetting::Confirmed(SettingValueId(current.to_owned())),
            cause: Some(cause),
        })
    );
    send_prompt(state, &input(run.generation, 9));
}

#[test]
fn t4_unoffered_saved_mode_still_commits_with_not_offered_delta() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let prepare = apply(
        &mut state,
        OwnerEvent::Wire(SessionWireResult::Restore {
            run,
            outcome: WireOutcome::Confirmed(RestoreLoadReadback {
                catalogue: Arc::default(),
                router_auto_requested: false,
                mode: RestoredMode::NotOffered {
                    current: Some(SettingValueId("default".into())),
                },
                fast_on: None,
            }),
        }),
    );
    assert!(!prepare.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Wire(SessionWireRequest::RestoreMode { .. })
    )));
    assert_usable_with_mode_delta(&mut state, run, "default", ObservationCause::NotOffered);
}

fn report_restore_mode(
    state: &mut SessionOwnerState,
    observation: RestoreModeOutcome,
) -> RunAttempt<LoadAttempt> {
    let run = open_wire(state, 1);
    let application = SettingApplication(uuid(2));
    let authorized = apply(
        state,
        OwnerEvent::Wire(SessionWireResult::Restore {
            run,
            outcome: WireOutcome::Confirmed(RestoreLoadReadback {
                catalogue: Arc::default(),
                router_auto_requested: false,
                mode: RestoredMode::Reapply {
                    application,
                    value: SettingValueId("auto".into()),
                },
                fast_on: None,
            }),
        }),
    );
    let setting = authorized
        .into_iter()
        .find_map(|effect| match effect {
            OwnerEffect::Wire(SessionWireRequest::RestoreMode { setting, .. }) => Some(setting),
            _ => None,
        })
        .expect("exact setting application");
    apply(
        state,
        OwnerEvent::Wire(SessionWireResult::RestoreMode {
            run,
            setting,
            outcome: observation,
            catalogue: None,
        }),
    );
    run
}

#[test]
fn t4_refused_restore_setting_still_commits_with_refusal_and_observed_value() {
    let mut state = fresh();
    let run = report_restore_mode(
        &mut state,
        RestoreModeOutcome::Refused {
            current: Some(SettingValueId("ask".into())),
        },
    );
    assert_usable_with_mode_delta(&mut state, run, "ask", ObservationCause::Refused);
}

#[test]
fn t4_different_confirmed_restore_setting_still_commits_with_its_actual_value() {
    let mut state = fresh();
    let run = report_restore_mode(
        &mut state,
        RestoreModeOutcome::ReportedOther(SettingValueId("plan".into())),
    );
    assert_usable_with_mode_delta(&mut state, run, "plan", ObservationCause::ReportedOther);
}

#[test]
fn t4_fallback_restore_setting_still_commits_with_its_actual_value() {
    let mut state = fresh();
    let run = report_restore_mode(
        &mut state,
        RestoreModeOutcome::FellBack(SettingValueId("default".into())),
    );
    assert_usable_with_mode_delta(&mut state, run, "default", ObservationCause::FellBack);
}

#[test]
fn t2_explicit_steer_on_idle_uses_the_prompt_path_and_preserves_the_input() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let request = input(run.generation, 2);
    let effects = command(
        &mut state,
        SessionOwnerCommand::Steer {
            input: request.clone(),
            supported: true,
        },
    );
    assert!(
        effects.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
            request.clone()
        )))
    );
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        OwnerEffect::Store(SessionStoreRequest::MarkSteerDispatch(_))
    )));

    let marked = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::Dispatch {
            input: request.clone(),
            outcome: EffectOutcome::Applied,
        }),
    );
    let token = authorization(&marked);
    assert_eq!(token.target, AuthorizationTarget::NewTurn);
    assert_eq!(token.input_id, request.input);
}

#[test]
fn t2_correlated_provider_started_turn_is_applied_and_eligible_without_another_send() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let foreground = input(run.generation, 2);
    let first = send_prompt(&mut state, &foreground);
    let request = pending_steer(&mut state, &foreground);
    let marked = apply(&mut state, steer_marked(&request));
    let steer = authorization(&marked);
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Taken(steer.clone())),
    );
    let observed_turn = TurnId(uuid(4));
    let response = OwnerEvent::Dispatch(DispatchResult::SteerStartedNewTurn {
        authorization: steer,
        turn: observed_turn,
    });
    let effects = apply_pending(&mut state, response.clone());
    assert!(effects.iter().any(|effect| matches!(effect,
        OwnerEffect::Store(SessionStoreRequest::RecordTerminal { reply, .. })
            if **reply == SessionOwnerReply::SteerStarted { input: request.clone(), turn: observed_turn })));
    assert!(effects.contains(&OwnerEffect::CallbackMembership(
        TurnMembership::ObservedEligible {
            run: run.generation,
            turn: observed_turn,
        }
    )));
    assert!(
        effects.contains(&OwnerEffect::ProviderStartedTurn(OwnerInput {
            turn: observed_turn,
            ..request
        }))
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, OwnerEffect::Authorize { .. })),
        "the provider already applied the steer input"
    );
    assert_eq!(
        state.snapshot().turn,
        SessionTurn::Running {
            turn: observed_turn
        }
    );
    assert_late(&state, response);
    let prior_terminal = apply_pending(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::PromptFinished {
            authorization: first,
            outcome: WireOutcome::Confirmed(PromptCompletion {
                stop_reason: session_event_model::StopReason::EndTurn,
                local_cause: None,
                response: String::new(),
            }),
        }),
    );
    assert!(prior_terminal.iter().any(|effect| matches!(effect,
        OwnerEffect::Store(SessionStoreRequest::RecordTerminal { reply, .. })
            if matches!(reply.as_ref(), SessionOwnerReply::PromptCompleted { input, .. } if input == &foreground)
    )));
    assert!(prior_terminal.iter().any(|effect| matches!(effect,
        OwnerEffect::Publish(PublicationBatch::Commit { facts, .. }) if facts.iter().any(|fact|
            matches!(fact, PublicationFact::Live(SessionPublication::PreviousTurnEnded { turn, .. }) if *turn == foreground.turn))
    )));
    assert_eq!(
        state.snapshot().turn,
        SessionTurn::Running {
            turn: observed_turn
        }
    );
    apply(
        &mut state,
        OwnerEvent::ObservedTurnEnded {
            run: run.generation,
            turn: observed_turn,
            outcome: session_event_model::TurnOutcome::Lost {
                reason: session_event_model::TurnLostReason::EndNotObservable,
            },
        },
    );
    assert_eq!(state.snapshot().turn, SessionTurn::Idle);
}
