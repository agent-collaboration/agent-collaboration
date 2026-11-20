use super::*;
use crate::tool_ownership::{SessionlessCallbackOwner, sessionless_callback_owner};

fn open_wire(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    let run = start_restore(state, number);
    let effects = read_setup(state, run);
    assert!(
        effects.contains(&SessionOwnerEffect::Wire(SessionWireRequest::Restore {
            run,
            via: RestoreVia::Load
        }))
    );
    run
}

fn expire_restore(
    state: &mut SessionOwnerState,
    run: RunAttempt<LoadAttempt>,
    wait: RestoreWait,
) -> Vec<SessionOwnerEffect> {
    apply(
        state,
        SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Restore { run, wait }),
    )
}

fn assert_late(state: &SessionOwnerState, event: SessionOwnerEvent) {
    assert!(matches!(
        state.decide(event),
        Err(SessionOwnerRejection::IgnoredLate)
    ));
}

fn assert_retired(state: &SessionOwnerState, generation: SessionGeneration) {
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::NotLoaded { last: generation }
    );
}

fn send_prompt(state: &mut SessionOwnerState, prompt: &OwnerInput) {
    enqueue_prompt(state, prompt);
    let effects = apply(state, mark_prompt(prompt));
    assert!(
        effects.contains(&SessionOwnerEffect::Wire(SessionWireRequest::Prompt(
            prompt.clone()
        )))
    );
}

fn staged_tool(effects: &[SessionOwnerEffect], generation: SessionGeneration) -> bool {
    effects.contains(&SessionOwnerEffect::StageReplayTool {
        generation,
        tool_id: "provider-tool-id".to_owned(),
    })
}

#[test]
fn cold_auto_input_waiting_on_a_silent_restore_is_never_sent_after_reload() {
    let mut state = SessionOwnerState::fresh();
    let first = open_wire(&mut state, 1);
    let old_input = input(first.generation, 2);
    let admitted = enqueue_prompt(&mut state, &old_input);
    assert!(
        admitted.contains(&SessionOwnerEffect::ArmDeadline(SessionDeadline::Prompt {
            input: old_input.clone(),
            wait: InputWait::Admission
        }))
    );

    let failed = expire_restore(&mut state, first, RestoreWait::Wire);
    assert!(failed.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::InputFinished {
            input: old_input.clone(),
            remote: RemoteEffect::None
        }
    )));
    assert!(failed.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::RestoreFailed {
            run: first,
            stage: RestoreFailureStage::Wire,
            remote: RemoteEffect::Unknown
        }
    )));
    let next = restore_live(&mut state, 3);
    assert_ne!(first.generation, next.generation);
    assert_late(&state, mark_prompt(&old_input));
    assert_eq!(state.turn(), SessionTurn::Idle);
}

#[test]
fn unanswered_restore_retires_its_provisional_tool_observation() {
    let mut state = SessionOwnerState::fresh();
    let first = open_wire(&mut state, 1);
    assert!(staged_tool(
        &apply(
            &mut state,
            tool_update(first.generation, ToolObservation::Announcement)
        ),
        first.generation
    ));

    expire_restore(&mut state, first, RestoreWait::Wire);
    assert_eq!(
        state.tools.get("provider-tool-id"),
        Some(&ToolOwnership::Retired {
            generation: first.generation
        })
    );
    let next = restore_live(&mut state, 2);
    assert!(!tool_forwarded(
        &apply(&mut state, callback(next.generation)),
        next.generation
    ));
}

#[test]
fn retired_replay_id_does_not_fall_back_to_a_subsequent_live_turn() {
    let mut state = SessionOwnerState::fresh();
    let run = open_wire(&mut state, 1);
    apply(
        &mut state,
        tool_update(run.generation, ToolObservation::Announcement),
    );
    apply(&mut state, restore_reply(run));
    let first = input(run.generation, 2);
    send_prompt(&mut state, &first);
    assert!(tool_forwarded(
        &apply(&mut state, callback(run.generation)),
        run.generation
    ));

    end_turn(&mut state, &first);
    send_prompt(&mut state, &input(run.generation, 3));
    assert!(!tool_forwarded(
        &apply(&mut state, callback(run.generation)),
        run.generation
    ));
    let unseen = SessionOwnerEvent::RoutedTool(RoutedSessionToolEvent::RoutedCallback {
        generation: run.generation,
        tool_id: "unobserved-id".to_owned(),
    });
    assert!(
        apply(&mut state, unseen).contains(&SessionOwnerEffect::ForwardTool {
            generation: run.generation,
            tool_id: "unobserved-id".to_owned(),
        })
    );
}

#[test]
fn a_retired_tool_update_cannot_project_into_a_newer_idle_session() {
    let mut state = SessionOwnerState::fresh();
    let old = restore_live(&mut state, 1);
    apply(
        &mut state,
        tool_update(old.generation, ToolObservation::Announcement),
    );
    let close = start_close(&mut state, old.generation, 2);
    apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Confirmed(()),
        }),
    );
    let newer = restore_live(&mut state, 3);

    assert_late(
        &state,
        tool_update(old.generation, ToolObservation::Continuation),
    );
    let incorrectly_addressed =
        SessionOwnerEvent::RoutedTool(RoutedSessionToolEvent::RetiredUpdate {
            generation: newer.generation,
            tool_id: "provider-tool-id".to_owned(),
            item_exists: true,
        });
    assert!(!tool_forwarded(
        &apply(&mut state, incorrectly_addressed),
        newer.generation
    ));
    assert_eq!(state.turn(), SessionTurn::Idle);
}

#[test]
fn refreshed_replay_ownership_survives_a_late_end_for_the_older_tombstone() {
    let mut state = SessionOwnerState::fresh();
    let old = open_wire(&mut state, 1);
    apply(
        &mut state,
        tool_update(old.generation, ToolObservation::Announcement),
    );
    expire_restore(&mut state, old, RestoreWait::Wire);
    let new = open_wire(&mut state, 2);
    assert!(staged_tool(
        &apply(
            &mut state,
            tool_update(new.generation, ToolObservation::Announcement)
        ),
        new.generation
    ));

    assert_late(
        &state,
        SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Restore {
            run: old,
            wait: RestoreWait::Wire,
        }),
    );
    apply(&mut state, restore_reply(new));
    assert!(tool_forwarded(
        &apply(&mut state, callback(new.generation)),
        new.generation
    ));
}

#[test]
fn post_wire_publication_failure_reports_the_known_remote_restore_and_ends_cold_auto() {
    let mut state = SessionOwnerState::fresh();
    let run = open_wire(&mut state, 1);
    let old_input = input(run.generation, 2);
    enqueue_prompt(&mut state, &old_input);
    apply(&mut state, restore_reply(run));

    let failures = apply(
        &mut state,
        SessionOwnerEvent::Publication {
            run,
            outcome: EffectOutcome::Failed,
        },
    );
    assert!(failures.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::RestoreFailed {
            run,
            stage: RestoreFailureStage::Publication,
            remote: RemoteEffect::Applied,
        }
    )));
    assert!(failures.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::InputFinished {
            input: old_input.clone(),
            remote: RemoteEffect::None
        }
    )));
    restore_live(&mut state, 3);
    assert_late(&state, mark_prompt(&old_input));
}

#[test]
fn shared_tool_id_is_rejected_at_routing_even_after_one_owner_retires() {
    let mut first = SessionOwnerState::fresh();
    let mut second = SessionOwnerState::fresh();
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
    expire_restore(&mut first, run_a, RestoreWait::Wire);

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
    // An unambiguous routing result can reach the real session owner.
    let one = sessionless_callback_owner(std::iter::once((&second_id, b)));
    assert_eq!(one, SessionlessCallbackOwner::OwnedBy(&second_id));
    assert!(tool_forwarded(
        &apply(&mut second, callback(run_b.generation)),
        run_b.generation
    ));
}

#[test]
fn failed_host_activation_retires_the_exact_committed_route() {
    let mut state = SessionOwnerState::fresh();
    let run = open_wire(&mut state, 1);
    assert!(publication_for(
        run.generation,
        &apply(&mut state, restore_reply(run))
    ));

    let failed_publication = SessionOwnerEvent::Publication {
        run,
        outcome: EffectOutcome::Failed,
    };
    let effects = apply(&mut state, failed_publication.clone());
    assert_retired(&state, run.generation);
    assert!(effects.contains(&SessionOwnerEffect::RetireRoute(run.generation)));
    let replacement = restore_live(&mut state, 2);
    assert_late(&state, failed_publication);
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::Live {
            generation: replacement.generation
        }
    );
}

#[test]
fn a_refused_close_cannot_reopen_a_restore_it_has_already_displaced() {
    let mut state = SessionOwnerState::fresh();
    let restoring = open_wire(&mut state, 1);
    let close = start_close(&mut state, restoring.generation, 2);
    let restore_failed = SessionOwnerEvent::Wire(SessionWireResult::Restore {
        run: restoring,
        outcome: WireOutcome::Refused,
    });
    assert_late(&state, restore_failed);

    apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Refused,
        }),
    );
    assert_retired(&state, restoring.generation);
    let retry = restore_live(&mut state, 3);
    send_prompt(&mut state, &input(retry.generation, 4));
}

#[test]
fn input_waits_until_the_owner_commits_and_failed_publication_does_not_wait_for_a_turn() {
    let mut state = SessionOwnerState::fresh();
    let run = open_wire(&mut state, 1);
    let waiting = input(run.generation, 2);
    let effects = enqueue_prompt(&mut state, &waiting);
    assert_eq!(
        effects,
        vec![SessionOwnerEffect::ArmDeadline(SessionDeadline::Prompt {
            input: waiting.clone(),
            wait: InputWait::Admission
        })]
    );
    let committed = apply(&mut state, restore_reply(run));
    assert!(publication_for(run.generation, &committed));
    assert!(committed.contains(&SessionOwnerEffect::Store(
        SessionStoreRequest::MarkDispatch(waiting.clone())
    )));
    assert!(
        apply(&mut state, mark_prompt(&waiting)).contains(&SessionOwnerEffect::Wire(
            SessionWireRequest::Prompt(waiting.clone())
        ))
    );

    let retired = apply(
        &mut state,
        SessionOwnerEvent::Publication {
            run,
            outcome: EffectOutcome::Failed,
        },
    );
    assert_retired(&state, run.generation);
    assert!(retired.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::InputFinished {
            input: waiting,
            remote: RemoteEffect::Unknown
        }
    )));
    assert!(!retired.iter().any(|effect| matches!(
        effect,
        SessionOwnerEffect::Publish(SessionPublication::TurnEnded { idle: true, .. })
    )));
}

#[test]
fn definite_fast_retry_without_readback_preserves_the_previous_unknown_write() {
    let mut state = SessionOwnerState::fresh();
    let run = restore_live(&mut state, 1);
    let fast = |number| OwnerSetting {
        change: RunAttempt {
            generation: run.generation,
            attempt: SettingApplication(uuid(number)),
        },
        kind: ProviderSettingKind::Fast,
    };
    let off = || SettingValueId("off".to_owned());
    command(
        &mut state,
        SessionOwnerCommand::Set {
            setting: fast(2),
            value: off(),
        },
    );
    apply(
        &mut state,
        SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Setting {
            setting: fast(2),
            wait: SettingWait::Wire,
        }),
    );
    command(
        &mut state,
        SessionOwnerCommand::Set {
            setting: fast(3),
            value: off(),
        },
    );
    apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Setting {
            setting: fast(3),
            observation: SettingObservation::Refused { current: None },
        }),
    );

    let blocked = state.decide(SessionOwnerEvent::Command(SessionOwnerCommand::Prompt {
        input: input(run.generation, 4),
        admission: PromptAdmission::Direct,
    }));
    assert!(matches!(
        blocked,
        Err(SessionOwnerRejection::Refused(
            SessionOwnerRefusal::SettingsUnresolved
        ))
    ));
    assert!(matches!(
        state.decide(SessionOwnerEvent::Command(
            SessionOwnerCommand::AcceptSettings {
                generation: run.generation
            }
        )),
        Err(SessionOwnerRejection::Refused(_))
    ));
    command(
        &mut state,
        SessionOwnerCommand::Set {
            setting: fast(5),
            value: off(),
        },
    );
    apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Setting {
            setting: fast(5),
            observation: SettingObservation::Applied(off()),
        }),
    );
    apply(
        &mut state,
        SessionOwnerEvent::Store(SessionStoreResult::ConfirmedSetting {
            setting: fast(5),
            outcome: EffectOutcome::Applied,
        }),
    );
    send_prompt(&mut state, &input(run.generation, 6));
}

#[test]
fn an_auto_input_admitted_on_a_cannot_send_its_late_dispatch_mark_on_b() {
    let mut state = SessionOwnerState::fresh();
    let first = open_wire(&mut state, 1);
    let waiting = input(first.generation, 2);
    enqueue_prompt(&mut state, &waiting);
    let effects = apply(&mut state, restore_reply(first));
    assert!(effects.contains(&SessionOwnerEffect::Store(
        SessionStoreRequest::MarkDispatch(waiting.clone())
    )));
    let close = start_close(&mut state, first.generation, 3);
    apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Confirmed(()),
        }),
    );
    let second = restore_live(&mut state, 4);

    assert_late(&state, mark_prompt(&waiting));
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::Live {
            generation: second.generation
        }
    );
    assert_eq!(state.turn(), SessionTurn::Idle);
}

#[test]
fn unanswered_competing_close_ends_locally_and_allows_a_new_restore() {
    let mut state = SessionOwnerState::fresh();
    let restoring = open_wire(&mut state, 1);
    let close = start_close(&mut state, restoring.generation, 2);
    assert_late(&state, restore_reply(restoring));

    let expired = apply(
        &mut state,
        SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Close(close)),
    );
    assert_retired(&state, restoring.generation);
    assert!(expired.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::CloseFinished {
            run: close,
            outcome: CloseOutcome::OutcomeUnknown
        }
    )));
    assert!(
        expired.contains(&SessionOwnerEffect::Publish(SessionPublication::Unloaded(
            close.generation
        )))
    );
    let replacement = restore_live(&mut state, 3);
    assert_late(
        &state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Confirmed(()),
        }),
    );
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::Live {
            generation: replacement.generation
        }
    );
}

#[test]
fn close_winning_before_restore_commit_releases_no_idle_publication() {
    let mut state = SessionOwnerState::fresh();
    let restoring = open_wire(&mut state, 1);
    let close = start_close(&mut state, restoring.generation, 2);

    assert_late(&state, restore_reply(restoring));
    assert_late(
        &state,
        SessionOwnerEvent::Publication {
            run: restoring,
            outcome: EffectOutcome::Applied,
        },
    );
    let closed = apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Confirmed(()),
        }),
    );
    assert!(!publication_for(restoring.generation, &closed));
    assert_retired(&state, restoring.generation);
}

#[test]
fn stalled_host_acknowledgement_does_not_hold_loading_or_strand_the_committed_run() {
    let mut state = SessionOwnerState::fresh();
    let restoring = open_wire(&mut state, 1);
    let committed = apply(&mut state, restore_reply(restoring));
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::Live {
            generation: restoring.generation
        }
    );
    assert!(publication_for(restoring.generation, &committed));

    let due = expire_restore(&mut state, restoring, RestoreWait::Publication);
    assert_retired(&state, restoring.generation);
    assert!(due.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::RestoreFailed {
            run: restoring,
            stage: RestoreFailureStage::Publication,
            remote: RemoteEffect::Applied,
        }
    )));
    restore_live(&mut state, 2);
    assert_late(
        &state,
        SessionOwnerEvent::Publication {
            run: restoring,
            outcome: EffectOutcome::Applied,
        },
    );
}

#[test]
fn definite_close_refusal_on_live_leaves_the_run_usable() {
    let mut state = SessionOwnerState::fresh();
    let run = restore_live(&mut state, 1);
    let close = start_close(&mut state, run.generation, 2);

    let refused = apply(
        &mut state,
        SessionOwnerEvent::Wire(SessionWireResult::Close {
            run: close,
            outcome: WireOutcome::Refused,
        }),
    );
    assert_eq!(
        state.lifecycle(),
        SessionLifecycle::Live {
            generation: run.generation
        }
    );
    assert!(refused.contains(&SessionOwnerEffect::Reply(
        SessionOwnerReply::CloseFinished {
            run: close,
            outcome: CloseOutcome::Refused
        }
    )));
    assert_eq!(
        CloseOutcome::Refused.human_phrase(),
        "the provider refused the close"
    );
    assert_late(
        &state,
        SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Close(close)),
    );
    send_prompt(&mut state, &input(run.generation, 3));
}

#[test]
fn setting_cannot_overtake_an_input_waiting_for_its_dispatch_mark() {
    let mut state = SessionOwnerState::fresh();
    let run = restore_live(&mut state, 1);
    let prompt = input(run.generation, 2);
    let admitted = enqueue_prompt(&mut state, &prompt);
    assert!(admitted.contains(&SessionOwnerEffect::Store(
        SessionStoreRequest::MarkDispatch(prompt.clone())
    )));
    let set = SessionOwnerCommand::Set {
        setting: OwnerSetting {
            change: RunAttempt {
                generation: run.generation,
                attempt: SettingApplication(uuid(3)),
            },
            kind: ProviderSettingKind::Fast,
        },
        value: SettingValueId("off".to_owned()),
    };

    assert!(matches!(
        state.decide(SessionOwnerEvent::Command(set.clone())),
        Err(SessionOwnerRejection::Refused(
            SessionOwnerRefusal::EffectInProgress
        ))
    ));
    apply(&mut state, mark_prompt(&prompt));
    end_turn(&mut state, &prompt);
    assert!(command(&mut state, set).iter().any(|effect| matches!(
        effect,
        SessionOwnerEffect::Wire(SessionWireRequest::Set { .. })
    )));
}
