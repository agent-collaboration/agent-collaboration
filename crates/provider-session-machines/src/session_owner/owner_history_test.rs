use super::*;
use crate::tool_ownership::{SessionlessCallbackOwner, sessionless_callback_owner};

fn retired(state: &SessionOwnerState, generation: SessionGeneration) {
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::NotLoaded { last: generation }
    );
}

fn failed_prepare(run: RunAttempt<LoadAttempt>) -> OwnerEvent {
    OwnerEvent::Prepare(RestorePreparation::Failed {
        run,
        stage: PreparationStage::HostSetup,
    })
}

fn failed_restore(effects: &[OwnerEffect], run: RunAttempt<LoadAttempt>, remote: RemoteEffect) {
    assert!(effects.iter().any(|effect| matches!(effect,
        OwnerEffect::Reply(SessionOwnerReply::RestoreFinished(RestoreResult {
            attempt, phase: RestorePhase::Attempted, disposition: RestoreDisposition::Retired { remote: actual, .. }, ..
        })) if *attempt == run && *actual == remote
    )));
}

#[test]
fn cold_auto_input_waiting_on_a_silent_restore_is_never_sent_after_reload() {
    let mut state = fresh();
    let first = open_wire(&mut state, 1);
    let old = input(first.generation, 2);
    enqueue_prompt(&mut state, &old);

    let effects = deadline(&mut state, OwnerDeadline::RestoreWire(first));
    failed_restore(&effects, first, RemoteEffect::Unknown);
    assert!(
        effects.contains(&OwnerEffect::Reply(SessionOwnerReply::InputFinished {
            input: old.clone(),
            remote: RemoteEffect::None
        }))
    );
    let next = restore_live(&mut state, 3);
    assert_ne!(first.generation, next.generation);
    assert_late(&state, mark_prompt(&old));
}

#[test]
fn unanswered_restore_retires_its_provisional_tool_observation() {
    let mut state = fresh();
    let first = open_wire(&mut state, 1);
    let observed = apply(
        &mut state,
        tool_update(first.generation, ToolObservation::Announcement),
    );
    assert!(!tool_forwarded(&observed, first.generation));

    deadline(&mut state, OwnerDeadline::RestoreWire(first));
    assert_eq!(
        state.tools.get("provider-tool-id"),
        Some(&ToolOwnership::Retired {
            generation: first.generation
        })
    );
    let next = restore_live(&mut state, 2);
    let prompt = input(next.generation, 3);
    send_prompt(&mut state, &prompt);
    assert!(!tool_forwarded(
        &apply(&mut state, callback(next.generation, prompt.turn)),
        next.generation
    ));
}

#[test]
fn retired_replay_id_does_not_fall_back_to_a_subsequent_live_turn() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    apply(
        &mut state,
        tool_update(run.generation, ToolObservation::Announcement),
    );
    apply(&mut state, restore_reply(run));
    finish_preparation(&mut state, run);
    let first = input(run.generation, 2);
    let key = send_prompt(&mut state, &first);
    assert!(tool_forwarded(
        &apply(&mut state, callback(run.generation, first.turn)),
        run.generation
    ));

    end_turn(&mut state, key);
    let second = input(run.generation, 3);
    send_prompt(&mut state, &second);
    assert!(!tool_forwarded(
        &apply(&mut state, callback(run.generation, second.turn)),
        run.generation
    ));
    let unseen = OwnerEvent::RoutedTool(RoutedTool::Exact {
        run: run.generation,
        turn: second.turn,
        tool_id: "unobserved-id".to_owned(),
    });
    assert!(
        apply(&mut state, unseen).contains(&OwnerEffect::ForwardTool {
            run: run.generation,
            turn: Some(second.turn),
            tool_id: "unobserved-id".to_owned(),
        })
    );
}

#[test]
fn a_retired_tool_update_cannot_project_into_a_newer_idle_session() {
    let mut state = fresh();
    let previous = restore_live(&mut state, 1);
    apply(
        &mut state,
        tool_update(previous.generation, ToolObservation::Announcement),
    );
    let close = start_close(&mut state, previous.generation, 2);
    apply(&mut state, close_reply(close, WireOutcome::Confirmed(())));
    let newer = restore_live(&mut state, 3);

    assert_late(
        &state,
        tool_update(previous.generation, ToolObservation::Continuation),
    );
    apply(
        &mut state,
        OwnerEvent::RoutedTool(RoutedTool::ItemRetained {
            run: newer.generation,
            item_id: "provider-tool-id".to_owned(),
        }),
    );
    let continued = apply(
        &mut state,
        OwnerEvent::RoutedTool(RoutedTool::ExactIdle {
            run: newer.generation,
            item_id: "provider-tool-id".to_owned(),
        }),
    );
    assert!(!tool_forwarded(&continued, newer.generation));
}

#[test]
fn refreshed_replay_ownership_survives_a_late_end_for_the_older_tombstone() {
    let mut state = fresh();
    let old = open_wire(&mut state, 1);
    apply(
        &mut state,
        tool_update(old.generation, ToolObservation::Announcement),
    );
    deadline(&mut state, OwnerDeadline::RestoreWire(old));
    let new = open_wire(&mut state, 2);
    apply(
        &mut state,
        tool_update(new.generation, ToolObservation::Announcement),
    );

    assert_late(
        &state,
        OwnerEvent::Deadline(OwnerDeadline::RestoreWire(old)),
    );
    apply(&mut state, restore_reply(new));
    finish_preparation(&mut state, new);
    let prompt = input(new.generation, 3);
    send_prompt(&mut state, &prompt);
    assert!(tool_forwarded(
        &apply(&mut state, callback(new.generation, prompt.turn)),
        new.generation
    ));
}

#[test]
fn post_wire_prerequisite_failure_reports_the_remote_restore_and_ends_cold_auto() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let old = input(run.generation, 2);
    enqueue_prompt(&mut state, &old);
    apply(&mut state, restore_reply(run));

    let effects = apply(&mut state, failed_prepare(run));
    failed_restore(&effects, run, RemoteEffect::Applied);
    assert!(
        effects.contains(&OwnerEffect::Reply(SessionOwnerReply::InputFinished {
            input: old.clone(),
            remote: RemoteEffect::None
        }))
    );
    restore_live(&mut state, 3);
    assert_late(&state, mark_prompt(&old));
}

#[test]
fn shared_tool_id_is_rejected_at_routing_even_after_one_owner_retires() {
    let mut first = fresh();
    let mut second = fresh();
    let run_a = open_wire(&mut first, 1);
    let run_b = restore_live(&mut second, 2);
    apply(
        &mut first,
        tool_update(run_a.generation, ToolObservation::Announcement),
    );
    apply(
        &mut second,
        tool_update(run_b.generation, ToolObservation::Announcement),
    );
    deadline(&mut first, OwnerDeadline::RestoreWire(run_a));
    let prompt = input(run_b.generation, 3);
    send_prompt(&mut second, &prompt);

    let tool = "provider-tool-id";
    let a = first.tools.get(tool).expect("retired routing observation");
    let b = second
        .tools
        .get(tool)
        .expect("surviving routing observation");
    let first_id = "session-a";
    let second_id = "session-b";
    assert_eq!(
        sessionless_callback_owner([(&first_id, a), (&second_id, b)].into_iter()),
        SessionlessCallbackOwner::Refused
    );
    assert_eq!(
        sessionless_callback_owner(std::iter::once((&second_id, b))),
        SessionlessCallbackOwner::OwnedBy(&second_id)
    );
    assert!(tool_forwarded(
        &apply(&mut second, callback(run_b.generation, prompt.turn)),
        run_b.generation
    ));
}

#[test]
fn failed_host_preparation_retires_exact_route_without_an_activation_batch() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    apply(&mut state, restore_reply(run));

    let failed = apply(&mut state, failed_prepare(run));
    retired(&state, run.generation);
    assert!(failed.contains(&OwnerEffect::RetireRoute(run.generation)));
    assert!(publication_for(run.generation, &failed).is_none());
    let replacement = restore_live(&mut state, 2);
    assert_late(&state, failed_prepare(run));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live {
            generation: replacement.generation
        }
    );
}

#[test]
fn a_refused_close_cannot_reopen_a_restore_it_has_already_displaced() {
    let mut state = fresh();
    let restoring = open_wire(&mut state, 1);
    let close = start_close(&mut state, restoring.generation, 2);
    assert_late(&state, failed_prepare(restoring));

    let effects = apply(&mut state, close_reply(close, WireOutcome::Refused));
    retired(&state, restoring.generation);
    assert!(
        effects.contains(&OwnerEffect::Reply(SessionOwnerReply::CloseFinished {
            run: close,
            outcome: CloseOutcome::RefusedLoading
        }))
    );
    let retry = restore_live(&mut state, 3);
    send_prompt(&mut state, &input(retry.generation, 4));
}

#[test]
fn input_waits_for_owner_commit_and_failed_preparation_never_waits_for_a_turn() {
    let mut state = fresh();
    let run = open_wire(&mut state, 1);
    let waiting = input(run.generation, 2);
    assert!(enqueue_prompt(&mut state, &waiting).is_empty());
    let prepared = apply(&mut state, restore_reply(run));
    assert!(prepared.contains(&OwnerEffect::PrepareHost(run)));
    assert!(
        !prepared.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
            waiting.clone()
        )))
    );

    let failed = apply(&mut state, failed_prepare(run));
    retired(&state, run.generation);
    assert!(
        failed.contains(&OwnerEffect::Reply(SessionOwnerReply::InputFinished {
            input: waiting,
            remote: RemoteEffect::None
        }))
    );
}

#[test]
fn definite_fast_retry_without_readback_preserves_the_previous_unknown_write() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let fast = |number| OwnerSetting {
        change: RunAttempt {
            generation: run.generation,
            attempt: SettingApplication(uuid(number)),
        },
        kind: ProviderSettingKind::Fast,
    };
    let first = setting_authorized(&mut state, fast(2), 20);
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::SettingFinished {
            authorization: first,
            observation: SettingObservation::Unknown,
        }),
    );
    let retry = setting_authorized(&mut state, fast(3), 30);
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::SettingFinished {
            authorization: retry,
            observation: SettingObservation::Refused { current: None },
        }),
    );

    let blocked = decide(
        &state,
        OwnerEvent::Command(SessionOwnerCommand::Prompt {
            input: input(run.generation, 4),
            admission: PromptAdmission::Direct,
        }),
    );
    assert!(matches!(
        blocked,
        Err(SessionOwnerRejection::Refused(
            SessionOwnerRefusal::SettingsUnresolved
        ))
    ));
    assert!(matches!(
        decide(
            &state,
            OwnerEvent::Command(SessionOwnerCommand::AcceptSettings {
                generation: run.generation
            })
        ),
        Err(SessionOwnerRejection::Refused(_))
    ));
    let confirmed = setting_authorized(&mut state, fast(5), 50);
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::SettingFinished {
            authorization: confirmed,
            observation: SettingObservation::Applied(SettingValueId("off".to_owned())),
        }),
    );
    apply(
        &mut state,
        OwnerEvent::Store(SessionStoreResult::ConfirmedSetting {
            setting: fast(5),
            outcome: EffectOutcome::Applied,
        }),
    );
    send_prompt(&mut state, &input(run.generation, 6));
}

#[test]
fn an_auto_input_admitted_on_a_cannot_send_its_late_dispatch_mark_on_b() {
    let mut state = fresh();
    let first = open_wire(&mut state, 1);
    let waiting = input(first.generation, 2);
    enqueue_prompt(&mut state, &waiting);
    apply(&mut state, restore_reply(first));
    assert!(
        finish_preparation(&mut state, first).contains(&OwnerEffect::Store(
            SessionStoreRequest::MarkDispatch(waiting.clone())
        ))
    );
    let close = start_close(&mut state, first.generation, 3);
    apply(&mut state, close_reply(close, WireOutcome::Confirmed(())));
    let second = restore_live(&mut state, 4);

    assert_late(&state, mark_prompt(&waiting));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live {
            generation: second.generation
        }
    );
    assert_eq!(state.snapshot().turn, SessionTurn::Idle);
}

#[test]
fn unanswered_competing_close_ends_locally_and_allows_a_new_restore() {
    let mut state = fresh();
    let restoring = open_wire(&mut state, 1);
    let close = start_close(&mut state, restoring.generation, 2);
    assert_late(&state, restore_reply(restoring));

    let expired = deadline(&mut state, OwnerDeadline::Close(close));
    retired(&state, restoring.generation);
    assert!(
        expired.contains(&OwnerEffect::Reply(SessionOwnerReply::CloseFinished {
            run: close,
            outcome: CloseOutcome::OutcomeUnknown
        }))
    );
    let replacement = restore_live(&mut state, 3);
    assert_late(&state, close_reply(close, WireOutcome::Confirmed(())));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live {
            generation: replacement.generation
        }
    );
}

#[test]
fn close_winning_before_restore_commit_releases_no_idle_publication() {
    let mut state = fresh();
    let restoring = open_wire(&mut state, 1);
    apply(&mut state, restore_reply(restoring));
    apply(
        &mut state,
        OwnerEvent::Prepare(RestorePreparation::RouteReady(restoring)),
    );
    let close = start_close(&mut state, restoring.generation, 2);

    assert_late(&state, host_ready(restoring));
    let effects = apply(&mut state, close_reply(close, WireOutcome::Confirmed(())));
    assert!(publication_for(restoring.generation, &effects).is_none());
    retired(&state, restoring.generation);
}

#[test]
fn stalled_host_work_expires_and_unapplied_committed_history_rebases_without_retirement() {
    let mut state = fresh();
    let first = open_wire(&mut state, 1);
    apply(&mut state, restore_reply(first));
    let expired = deadline(&mut state, OwnerDeadline::HostPrepare(first));
    failed_restore(&expired, first, RemoteEffect::Applied);
    retired(&state, first.generation);
    assert_late(&state, host_ready(first));

    let newer = open_wire(&mut state, 2);
    apply(&mut state, restore_reply(newer));
    let commit = finish_preparation(&mut state, newer);
    let batch = publication_for(newer.generation, &commit).expect("commit batch");
    let expired = deadline(&mut state, OwnerDeadline::Publication(batch));
    assert_eq!(
        state.snapshot().lifecycle,
        SessionLifecycle::Live {
            generation: newer.generation
        }
    );
    assert!(expired.iter().any(|effect| matches!(effect, OwnerEffect::Arm { deadline: OwnerDeadline::Rebase(id), .. } if *id == batch)));
    let rebase = deadline(&mut state, OwnerDeadline::Rebase(batch));
    assert!(rebase.iter().any(|effect| matches!(effect, OwnerEffect::Publish(PublicationBatch::Rebase { baseline, .. }) if baseline.lifecycle == state.snapshot().lifecycle)));
}

#[test]
fn setting_cannot_overtake_an_input_waiting_for_its_dispatch_mark() {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let prompt = input(run.generation, 2);
    let effects = enqueue_prompt(&mut state, &prompt);
    assert!(
        effects.contains(&OwnerEffect::Store(SessionStoreRequest::MarkDispatch(
            prompt.clone()
        )))
    );
    let setting = OwnerSetting {
        change: RunAttempt {
            generation: run.generation,
            attempt: SettingApplication(uuid(3)),
        },
        kind: ProviderSettingKind::Fast,
    };

    assert!(matches!(
        decide(&state, OwnerEvent::Command(setting_command(setting, 3))),
        Err(SessionOwnerRejection::Refused(
            SessionOwnerRefusal::EffectInProgress
        ))
    ));
    let key = authorization(&apply(&mut state, mark_prompt(&prompt)));
    apply(
        &mut state,
        OwnerEvent::Dispatch(DispatchResult::Taken(key.clone())),
    );
    end_turn(&mut state, key);
    setting_authorized(&mut state, setting, 3);
}
