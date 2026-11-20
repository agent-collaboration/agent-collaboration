use super::*;

fn close_outcome(effects: &[OwnerEffect], run: RunAttempt<CloseAttempt>, outcome: CloseOutcome) {
    assert!(
        effects.contains(&OwnerEffect::Reply(SessionOwnerReply::CloseFinished {
            run,
            outcome
        }))
    );
}

#[test]
fn t7_confirmed_close_is_applied_and_releases_the_run() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply(&mut state, close_reply(closing, WireOutcome::Confirmed(())));
    close_outcome(&effects, closing, CloseOutcome::Confirmed);
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
}

#[test]
fn t7_router_only_close_sends_no_remote_close() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = RunAttempt {
        generation: run.generation,
        attempt: CloseAttempt(uuid(2)),
    };
    let effects = command(
        &mut state,
        SessionOwnerCommand::CloseWithCapability(CloseRequest {
            run: closing,
            provider_supports_close: false,
        }),
    );
    close_outcome(&effects, closing, CloseOutcome::RouterOnly);
    assert!(!effects.contains(&OwnerEffect::Wire(SessionWireRequest::Close(closing))));
    assert_eq!(
        state.snapshot().projection.state,
        session_event_model::SessionState::Closed
    );
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
}

#[test]
fn t7_drain_expiry_reports_close_known_unsent_even_when_cancel_was_sent() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let prompt = input(run.generation, 2);
    send_prompt(&mut state, &prompt);
    let closing = RunAttempt {
        generation: run.generation,
        attempt: CloseAttempt(uuid(3)),
    };
    let effects = command(
        &mut state,
        SessionOwnerCommand::Close {
            generation: closing.generation,
            attempt: closing.attempt,
        },
    );
    let cancel = authorization(&effects);
    assert_eq!(
        cancel.target,
        AuthorizationTarget::ExistingTurn { turn: prompt.turn }
    );
    assert!(effects.contains(&OwnerEffect::DrainCancel(cancel.clone())));
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::CancelDrainFinished(cancel.clone())),
    );
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::CancelTaken {
            authorization: cancel,
            drain: CancelDrainReport::Drained,
        }),
    );
    assert!(!effects.contains(&OwnerEffect::Wire(SessionWireRequest::Close(closing))));
    let effects = deadline(&mut state, OwnerDeadline::Close(closing));
    close_outcome(&effects, closing, CloseOutcome::DrainExpired);
    assert!(!effects.contains(&OwnerEffect::Wire(SessionWireRequest::Close(closing))));
}

#[test]
fn t7_definite_close_refusal_on_live_leaves_the_run_usable() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply(&mut state, close_reply(closing, WireOutcome::Refused));
    close_outcome(&effects, closing, CloseOutcome::RefusedLive);
    assert_eq!(
        CloseOutcome::RefusedLive.human_phrase(),
        "the close was refused; the session is still open"
    );
    assert_late(&state, OwnerEvent::Deadline(OwnerDeadline::Close(closing)));
    send_prompt(&mut state, &input(run.generation, 3));
}

#[test]
fn t7_definite_close_refusal_during_loading_leaves_not_loaded() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply(&mut state, close_reply(closing, WireOutcome::Refused));
    close_outcome(&effects, closing, CloseOutcome::RefusedLoading);
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
}

#[test]
fn t7_indeterminate_sent_close_releases_local_run_and_keeps_unknown_effect() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply(&mut state, close_reply(closing, WireOutcome::Unknown));
    close_outcome(&effects, closing, CloseOutcome::OutcomeUnknown);
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
    assert!(restore_live(&mut state, 3).generation > run.generation);
}

#[test]
fn t2_revoke_and_take_report_distinct_dispositions() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let prompt = input(run.generation, 2);
    enqueue_prompt(&mut state, &prompt);
    let key = authorization(&apply(&mut state, mark_prompt(&prompt)));
    assert_eq!(state.snapshot().turn, SessionTurn::Idle);
    let expiring = deadline(&mut state, OwnerDeadline::Enqueue(key.clone()));
    assert!(expiring.contains(&OwnerEffect::Revoke {
        authorization: key.clone(),
        reason: RevokeReason::EnqueueExpired
    }));
    let mut taken = state.clone();
    let revoked = apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Revoked(key.clone())),
    );
    assert!(
        revoked.contains(&OwnerEffect::Store(SessionStoreRequest::UndoDispatch(
            prompt.clone()
        )))
    );
    assert_late(
        &state,
        OwnerEvent::Dispatch(DispatchResult::Taken(key.clone())),
    );
    let effects = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::UndoDispatch {
            input: prompt.clone(),
            outcome: EffectOutcome::Applied,
        }),
    );
    assert!(
        effects.contains(&OwnerEffect::Reply(SessionOwnerReply::InputUnsent {
            input: prompt.clone(),
            reason: RevokeReason::EnqueueExpired
        }))
    );

    apply(
        &mut taken,
        OwnerEvent::Dispatch(DispatchResult::AlreadyTaken(key.clone())),
    );
    assert_eq!(
        taken.snapshot().turn,
        SessionTurn::Running { turn: prompt.turn }
    );
    assert_late(
        &taken,
        OwnerEvent::Deadline(OwnerDeadline::Enqueue(key.clone())),
    );
    let finished = end_turn(&mut taken, key);
    assert!(
        finished.contains(&OwnerEffect::Reply(SessionOwnerReply::PromptCompleted {
            input: prompt,
            completion: PromptCompletion {
                stop_reason: session_event_model::StopReason::EndTurn,
                local_cause: None,
                response: String::new(),
            },
        }))
    );
}

#[test]
fn t7_unloaded_run_on_lost_connection_can_restore_only_after_new_epoch() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let old = state.snapshot().owner;
    apply(&mut state, OwnerEvent::ConnectionLost);
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
    assert!(matches!(
        decide(
            &state,
            OwnerEvent::Command(SessionOwnerCommand::Restore {
                attempt: LoadAttempt(uuid(2)),
                via: RestoreVia::Load
            })
        ),
        Err(SessionOwnerRejection::Refused(_))
    ));
    let owner = OwnerAddress {
        connection_epoch: ConnectionEpoch(uuid(3)),
        ..old
    };
    state = state
        .decide(OwnerMailboxEvent {
            owner,
            event: OwnerEvent::ConnectionAttached,
        })
        .expect("new epoch attached")
        .next;
    assert!(restore_live(&mut state, 4).generation > run.generation);
    assert!(matches!(
        state.decide(OwnerMailboxEvent {
            owner: old,
            event: OwnerEvent::ConnectionLost
        }),
        Err(SessionOwnerRejection::IgnoredLate)
    ));
}

#[test]
fn t1_create_is_not_live_until_the_registry_adopts_the_provider_id() {
    let mut state = fresh();
    let operation = uuid(1);
    command(&mut state, SessionOwnerCommand::Create { operation });
    assert_late(
        &state,
        OwnerEvent::Create(CreateProgress::Adopted {
            operation,
            facts: Box::default(),
        }),
    );
    let adoption = apply(
        &mut state,
        OwnerEvent::Create(CreateProgress::ProviderId {
            operation,
            provider_id: "provider-id".into(),
        }),
    );
    assert!(adoption.contains(&OwnerEffect::AdoptProviderId {
        operation,
        provider_id: "provider-id".into()
    }));
    assert!(matches!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded { .. }
    ));
    let mut duplicate = state.clone();
    assert!(
        apply(
            &mut duplicate,
            OwnerEvent::Create(CreateProgress::AlreadyOwned { operation })
        )
        .contains(&OwnerEffect::CreateSettled {
            operation,
            outcome: CreateOutcome::SessionIdInUse
        })
    );
    let admitted = apply(
        &mut state,
        OwnerEvent::Create(CreateProgress::Adopted {
            operation,
            facts: Box::default(),
        }),
    );
    let generation = SessionGeneration::from_number(1);
    assert!(admitted.contains(&OwnerEffect::CreateSettled {
        operation,
        outcome: CreateOutcome::Adopted(generation)
    }));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live { generation }
    );
    assert_late(
        &state,
        OwnerEvent::Create(CreateProgress::Adopted {
            operation,
            facts: Box::default(),
        }),
    );
}

#[test]
fn t4_dependent_input_receives_exact_restore_result_before_its_unsent_terminal_offer() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let prompt = input(run.generation, 2);
    enqueue_prompt(&mut state, &prompt);
    let effects = deadline(&mut state, OwnerDeadline::RestoreWire(run));
    let report = effects.iter().position(|effect| matches!(effect, OwnerEffect::RestoreSettled { operation, result }
        if *operation == prompt.operation && result.attempt == run && result.phase == RestorePhase::Attempted))
        .expect("attempt result for the dependent operation");
    let unsent = effects
        .iter()
        .position(|effect| {
            matches!(effect, OwnerEffect::Store(SessionStoreRequest::RecordTerminal { reply, .. })
            if **reply == SessionOwnerReply::InputUnsent {
                input: prompt.clone(),
                reason: RevokeReason::RestoreDidNotActivate,
            })
        })
        .expect("input is known unsent");
    assert!(report < unsent);
}

#[test]
fn t5_terminal_store_expiry_returns_observation_without_a_reread() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply_pending(&mut state, close_reply(closing, WireOutcome::Confirmed(())));
    let write = effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, .. }) => Some(*write),
            _ => None,
        })
        .expect("terminal offered to the lane");
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, OwnerEffect::Reply(_)))
    );
    let expired = deadline(&mut state, OwnerDeadline::TerminalStore(write));
    assert_eq!(
        expired,
        vec![OwnerEffect::ReplyUnsaved(
            SessionOwnerReply::CloseFinished {
                run: closing,
                outcome: CloseOutcome::Confirmed
            }
        )]
    );
    assert_late(
        &state,
        OwnerEvent::Store(SessionStoreResult::Terminal {
            write,
            outcome: EffectOutcome::Applied,
        }),
    );
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded {
            last: run.generation
        }
    );
}

#[test]
fn t5_terminal_store_settlement_survives_connection_reattachment() {
    let mut state = fresh();
    let live = restore_live(&mut state, 1);
    let closing = start_close(&mut state, live.generation, 2);
    let effects = apply_pending(&mut state, close_reply(closing, WireOutcome::Confirmed(())));
    let write = effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Store(SessionStoreRequest::RecordTerminal { write, reply })
                if matches!(reply.as_ref(), SessionOwnerReply::CloseFinished { .. }) =>
            {
                Some(*write)
            }
            _ => None,
        })
        .expect("close terminal write");
    let old_address = state.snapshot().owner;
    apply(&mut state, OwnerEvent::ConnectionLost);
    let new_address = OwnerAddress {
        connection_epoch: ConnectionEpoch(uuid(3)),
        ..old_address
    };
    state = state
        .decide(OwnerMailboxEvent {
            owner: new_address,
            event: OwnerEvent::ConnectionAttached,
        })
        .expect("replacement connection")
        .next;
    let deadline = OwnerDeadline::TerminalStore(write);
    assert!(state.has_pending_deadline(&deadline));
    let settled = state
        .decide(OwnerMailboxEvent {
            owner: old_address,
            event: OwnerEvent::Store(SessionStoreResult::Terminal {
                write,
                outcome: EffectOutcome::Applied,
            }),
        })
        .expect("same-incarnation terminal write can settle after reattachment");
    assert_eq!(settled.effects.owner, new_address);
    assert_eq!(
        settled.effects.effects,
        vec![OwnerEffect::Reply(SessionOwnerReply::CloseFinished {
            run: closing,
            outcome: CloseOutcome::Confirmed,
        })]
    );
    assert!(!settled.next.has_pending_deadline(&deadline));
}

#[test]
fn t6_refused_terminal_batch_rebases_without_a_later_business_event() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let closing = start_close(&mut state, run.generation, 2);
    let effects = apply(&mut state, close_reply(closing, WireOutcome::Confirmed(())));
    let closed_batch = effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Publish(PublicationBatch::Commit { id, facts })
                if facts.contains(&PublicationFact::Live(SessionPublication::Closed(
                    run.generation,
                ))) =>
            {
                Some(*id)
            }
            _ => None,
        })
        .expect("committed Closed batch");
    let refused = apply(
        &mut state,
        OwnerEvent::Publication(PublicationResult::Refused(closed_batch)),
    );
    assert!(refused.iter().any(|effect| matches!(effect, OwnerEffect::Arm { deadline: OwnerDeadline::Rebase(id), .. } if *id == closed_batch)));
    let rebasing = deadline(&mut state, OwnerDeadline::Rebase(closed_batch));
    let rebase = rebasing
        .into_iter()
        .find_map(|effect| match effect {
            OwnerEffect::Publish(PublicationBatch::Rebase {
                id,
                baseline,
                history,
            }) => {
                assert_eq!(history, HistoryAvailability::Unavailable);
                assert_eq!(
                    baseline.lifecycle,
                    SessionLifecycle::NotLoaded {
                        last: run.generation
                    }
                );
                Some(id)
            }
            _ => None,
        })
        .expect("rebase offer");
    assert_late(
        &state,
        OwnerEvent::Publication(PublicationResult::Applied(closed_batch)),
    );
    apply(
        &mut state,
        OwnerEvent::Publication(PublicationResult::Applied(rebase)),
    );
    assert_eq!(state.snapshot().publication, PublicationStatus::Current);
}

#[test]
fn t5_late_restore_mode_correlation_releases_only_its_own_fence() {
    let mut state = fresh();
    let first = open_wire(&mut state, 1);
    let application = SettingApplication(uuid(2));
    let effects = apply(
        &mut state,
        OwnerEvent::Wire(SessionWireResult::Restore {
            run: first,
            fast_on: None,
            outcome: WireOutcome::Confirmed(RestoredMode::Reapply {
                application,
                value: SettingValueId("auto".into()),
            }),
        }),
    );
    let setting = effects
        .iter()
        .find_map(|effect| match effect {
            OwnerEffect::Wire(SessionWireRequest::RestoreMode { setting, .. }) => Some(*setting),
            _ => None,
        })
        .expect("mode write correlation");
    deadline(&mut state, OwnerDeadline::RestoreMode(first));
    finish_preparation(&mut state, first);
    let close = start_close(&mut state, first.generation, 3);
    apply(&mut state, close_reply(close, WireOutcome::Confirmed(())));
    let next = restore_live(&mut state, 4);

    let current = state.snapshot();
    let wrong_run = RunAttempt {
        generation: next.generation,
        ..first
    };
    assert_late(
        &state,
        OwnerEvent::Wire(SessionWireResult::RestoreMode {
            run: wrong_run,
            setting,
            outcome: RestoreModeOutcome::Confirmed(SettingValueId("auto".into())),
        }),
    );
    assert_eq!(
        state
            .ports
            .restore_setting_fences
            .get(&ProviderSettingKind::Mode),
        Some(&(first, setting))
    );
    let released = apply(
        &mut state,
        OwnerEvent::Wire(SessionWireResult::RestoreMode {
            run: first,
            setting,
            outcome: RestoreModeOutcome::Confirmed(SettingValueId("auto".into())),
        }),
    );
    assert!(released.is_empty());
    assert_eq!(state.snapshot(), current);
    assert!(
        !state
            .ports
            .restore_setting_fences
            .contains_key(&ProviderSettingKind::Mode)
    );
}

#[test]
fn t2_definite_prompt_required_can_replace_the_steer_once_after_durable_unmark() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let foreground = input(run.generation, 2);
    let foreground_key = send_prompt(&mut state, &foreground);
    let steering = OwnerInput {
        turn: foreground.turn,
        ..input(run.generation, 3)
    };
    command(
        &mut state,
        SessionOwnerCommand::Steer {
            input: steering.clone(),
            supported: true,
        },
    );
    let marked = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::SteerDispatch {
            input: steering.clone(),
            outcome: EffectOutcome::Applied,
        }),
    );
    let old = authorization(&marked);
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Taken(old.clone())),
    );
    end_turn(&mut state, foreground_key);

    let replacement_turn = TurnId(uuid(4));
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::PromptRequired {
            authorization: old.clone(),
            replacement_turn,
        }),
    );
    let unmarked = apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::UndoDispatch {
            input: steering.clone(),
            outcome: EffectOutcome::Applied,
        }),
    );
    let replacement_input = OwnerInput {
        turn: replacement_turn,
        ..steering
    };
    assert!(
        unmarked.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
            replacement_input.clone()
        )))
    );
    let offered = apply(&mut state, mark_prompt(&replacement_input));
    let replacement_key = authorization(&offered);
    assert_eq!(replacement_key.input_id, old.input_id);
    assert_eq!(replacement_key.run, old.run);
    assert!(replacement_key.authorization_seq > old.authorization_seq);
    assert_late(
        &state,
        OwnerEvent::Dispatch(DispatchResult::PromptRequired {
            authorization: old,
            replacement_turn,
        }),
    );
}

#[test]
fn t4_restore_batches_replay_settings_and_idle_only_after_both_prerequisites() {
    use session_event_model::{SessionItem, SessionItemKind};
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let item = SessionItem {
        item_id: "replayed-item".into(),
        kind: SessionItemKind::UserMessage,
        text: Some("restored passage".into()),
    };
    assert!(
        apply(
            &mut state,
            OwnerEvent::RoutedTool(RoutedTool::Replay {
                run,
                item: item.clone()
            })
        )
        .is_empty()
    );
    let preparing = apply(&mut state, restore_reply(run));
    assert!(publication_for(run.generation, &preparing).is_none());
    assert!(apply(&mut state, host_ready(run)).is_empty());
    assert!(matches!(
        state.snapshot().lifecycle,
        SessionLifecycle::Loading { .. }
    ));

    let committed = apply(
        &mut state,
        OwnerEvent::Prepare(RestorePreparation::RouteReady(run)),
    );
    let batches: Vec<_> = committed
        .iter()
        .filter_map(|effect| match effect {
            OwnerEffect::Publish(PublicationBatch::Commit { facts, .. }) => Some(facts.as_slice()),
            _ => None,
        })
        .collect();
    assert!(matches!(batches.as_slice(), [[
        PublicationFact::ReplayBoundary(boundary),
        PublicationFact::Replay { run: replay_run, event: session_event_model::SessionEvent::ItemStarted { item: replayed } },
        PublicationFact::Capabilities { run: capability_run, .. },
        PublicationFact::Settings { run: settings_run, .. },
        PublicationFact::Live(SessionPublication::Activated { run: idle_run, .. }),
    ]] if *boundary == run && *replay_run == run && *replayed == item
        && *capability_run == run.generation && *settings_run == run.generation && *idle_run == run));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live {
            generation: run.generation
        }
    );
    assert_late(
        &state,
        OwnerEvent::RoutedTool(RoutedTool::Replay { run, item }),
    );
}
