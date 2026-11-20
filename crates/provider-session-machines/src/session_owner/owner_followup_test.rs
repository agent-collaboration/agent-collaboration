use super::*;

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

    deadline(&mut state, OwnerDeadline::Enqueue(cancel.clone()));
    let revoked = apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Revoked(cancel.clone())),
    );
    no_membership_change(&revoked);
    assert!(revoked.contains(&OwnerEffect::CancelNotSent {
        authorization: cancel,
        reason: RevokeReason::EnqueueExpired,
    }));
    assert_eq!(
        state.snapshot().turn,
        SessionTurn::Running {
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
            fast_on: None,
            outcome: WireOutcome::Confirmed(RestoredMode::NotOffered {
                current: Some(SettingValueId("default".into())),
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
            fast_on: None,
            outcome: WireOutcome::Confirmed(RestoredMode::Reapply {
                application,
                value: SettingValueId("auto".into()),
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
