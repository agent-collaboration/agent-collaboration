use super::*;
use crate::session_lifecycle_machine::CloseOrigin;
use hegel::generators as gs;

struct Mailbox {
    owner: SessionOwnerState,
    admitted: BTreeMap<InputId, SessionGeneration>,
    armed: Vec<SessionDeadline>,
    arrivals: Vec<SessionOwnerEvent>,
}

impl Mailbox {
    fn new() -> Self {
        Self {
            owner: SessionOwnerState::fresh(),
            admitted: BTreeMap::new(),
            armed: Vec::new(),
            arrivals: Vec::new(),
        }
    }

    fn deliver(&mut self, event: SessionOwnerEvent) {
        let before = self.owner.lifecycle();
        let waited_before = pending(&self.owner);
        let decision = self.owner.decide(event.clone());
        if let Ok(SessionOwnerCommit { next, effects }) = decision {
            if let SessionOwnerEvent::Command(
                SessionOwnerCommand::Prompt { input, .. }
                | SessionOwnerCommand::Steer { input, .. },
            ) = &event
            {
                self.admitted.insert(input.input.clone(), input.generation);
            }
            assert_live_commit(before, next.lifecycle(), &event);
            for effect in effects {
                self.observe(next.lifecycle(), effect);
            }
            self.owner = next;
        }

        let remaining = pending(&self.owner);
        assert!(
            remaining.iter().all(|wait| self.armed.contains(wait)),
            "(c) pending work has no armed deadline: {remaining:?}"
        );
        if let SessionOwnerEvent::DeadlineElapsed(wait) = event {
            assert!(
                !waited_before.contains(&wait) || !remaining.contains(&wait),
                "(c) fired deadline left its pending item: {wait:?}"
            );
        }
        if !matches!(before, SessionLifecycle::NotLoaded { .. })
            && let SessionLifecycle::NotLoaded { last } = self.owner.lifecycle()
        {
            let retry =
                self.owner
                    .decide(SessionOwnerEvent::Command(SessionOwnerCommand::Restore {
                        attempt: LoadAttempt(uuid(999)),
                        via: RestoreVia::Load,
                    }));
            assert!(
                matches!(retry, Ok(SessionOwnerCommit { next, .. })
                if matches!(next.lifecycle(), SessionLifecycle::Loading { generation, .. }
                    if generation.number() > last.number())),
                "(d) a retired generation barred a deliberate restore"
            );
        }
    }

    fn observe(&mut self, lifecycle: SessionLifecycle, effect: SessionOwnerEffect) {
        match effect {
            SessionOwnerEffect::ArmDeadline(wait) => {
                self.armed.push(wait.clone());
                self.arrivals.push(SessionOwnerEvent::DeadlineElapsed(wait));
            }
            SessionOwnerEffect::Wire(request) => {
                if let SessionWireRequest::Prompt(ref input)
                | SessionWireRequest::Steer(ref input) = request
                {
                    assert_eq!(
                        self.admitted.get(&input.input),
                        Some(&input.generation),
                        "(b) wire input used a different generation from admission"
                    );
                    assert_eq!(
                        lifecycle,
                        SessionLifecycle::Live {
                            generation: input.generation
                        },
                        "(b) wire input left a retired generation"
                    );
                }
                self.arrivals.extend(wire_replies(request));
            }
            SessionOwnerEffect::Store(request) => self.arrivals.extend(store_replies(request)),
            SessionOwnerEffect::Publish(SessionPublication::Activated { run, .. }) => {
                assert_eq!(
                    lifecycle,
                    SessionLifecycle::Live {
                        generation: run.generation
                    },
                    "(e) observed Idle without committed Live"
                );
                for outcome in [EffectOutcome::Applied, EffectOutcome::Failed] {
                    self.arrivals
                        .push(SessionOwnerEvent::Publication { run, outcome });
                }
            }
            SessionOwnerEffect::Publish(SessionPublication::TurnEnded {
                generation,
                idle: true,
                ..
            }) => {
                assert_eq!(
                    lifecycle,
                    SessionLifecycle::Live { generation },
                    "(e) observed Idle after a run retired"
                );
            }
            SessionOwnerEffect::Publish(_)
            | SessionOwnerEffect::Reply(_)
            | SessionOwnerEffect::RetireRoute(_)
            | SessionOwnerEffect::StageReplayTool { .. }
            | SessionOwnerEffect::ForwardTool { .. }
            | SessionOwnerEffect::CheckRetainedTool { .. } => {}
        }
    }
}

fn assert_live_commit(
    before: SessionLifecycle,
    after: SessionLifecycle,
    event: &SessionOwnerEvent,
) {
    let SessionLifecycle::Live { generation } = after else {
        return;
    };
    if before == after {
        return;
    }
    let committed = match before {
        SessionLifecycle::Loading {
            generation: loading,
            attempt,
            ..
        } => {
            let expected = RunAttempt {
                generation: loading,
                attempt,
            };
            loading == generation
                && match event {
                    SessionOwnerEvent::Wire(
                        SessionWireResult::Restore {
                            run,
                            outcome: WireOutcome::Confirmed(_),
                        }
                        | SessionWireResult::RestoreMode { run, .. },
                    )
                    | SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Restore {
                        run,
                        wait: RestoreWait::Mode,
                    }) => *run == expected,
                    _ => false,
                }
        }
        SessionLifecycle::Closing {
            generation: closing,
            from: CloseOrigin::Live,
            attempt,
            ..
        } => {
            closing == generation
                && matches!(event, SessionOwnerEvent::Wire(SessionWireResult::Close { run, outcome: WireOutcome::Refused })
                if *run == RunAttempt { generation, attempt })
        }
        _ => false,
    };
    assert!(
        committed,
        "(a) Live did not follow a matching owner commit: {before:?}, {event:?}, {after:?}"
    );
}

fn pending(state: &SessionOwnerState) -> Vec<SessionDeadline> {
    let mut waits = Vec::new();
    if let Some(restore) = state.restore {
        let (run, wait) = match restore {
            PendingRestore::Setup { run, .. } => (run, RestoreWait::Setup),
            PendingRestore::Wire { run, .. } => (run, RestoreWait::Wire),
            PendingRestore::Mode { run, .. } => (run, RestoreWait::Mode),
            PendingRestore::Publication { run, .. } => (run, RestoreWait::Publication),
        };
        waits.push(SessionDeadline::Restore { run, wait });
    }
    if let SessionLifecycle::Closing {
        generation,
        attempt,
        ..
    } = state.lifecycle()
    {
        waits.push(SessionDeadline::Close(RunAttempt {
            generation,
            attempt,
        }));
    }
    waits.extend(
        state
            .prompts
            .values()
            .map(|pending| SessionDeadline::Prompt {
                input: pending.input.clone(),
                wait: pending.wait,
            }),
    );
    if let Some(pending) = &state.steer {
        waits.push(SessionDeadline::Steer {
            input: pending.input.clone(),
            wait: pending.wait,
        });
    }
    if let Some((setting, wait)) = state.setting {
        waits.push(SessionDeadline::Setting { setting, wait });
    }
    waits
}

fn wire_replies(request: SessionWireRequest) -> Vec<SessionOwnerEvent> {
    let replies = [
        WireOutcome::Confirmed(()),
        WireOutcome::Refused,
        WireOutcome::Unknown,
    ];
    match request {
        SessionWireRequest::Restore { run, .. } => [
            WireOutcome::Confirmed(RestoredMode::Observed),
            WireOutcome::Confirmed(RestoredMode::Reapply(SettingValueId("auto".to_owned()))),
            WireOutcome::Refused,
            WireOutcome::Unknown,
        ]
        .into_iter()
        .map(|outcome| SessionWireResult::Restore { run, outcome })
        .map(SessionOwnerEvent::Wire)
        .collect(),
        SessionWireRequest::RestoreMode { run, .. } => replies
            .into_iter()
            .map(|outcome| SessionWireResult::RestoreMode { run, outcome })
            .map(SessionOwnerEvent::Wire)
            .collect(),
        SessionWireRequest::Close(run) => replies
            .into_iter()
            .map(|outcome| SessionWireResult::Close { run, outcome })
            .map(SessionOwnerEvent::Wire)
            .collect(),
        SessionWireRequest::Prompt(input) => replies
            .into_iter()
            .map(|outcome| SessionWireResult::Turn {
                input: input.clone(),
                outcome,
            })
            .map(SessionOwnerEvent::Wire)
            .collect(),
        SessionWireRequest::Steer(input) => replies
            .into_iter()
            .map(|outcome| SessionWireResult::Steer {
                input: input.clone(),
                outcome,
            })
            .map(SessionOwnerEvent::Wire)
            .collect(),
        SessionWireRequest::Set { setting, value } => [
            SettingObservation::Applied(value),
            SettingObservation::Refused { current: None },
            SettingObservation::Unknown,
        ]
        .into_iter()
        .map(|observation| SessionWireResult::Setting {
            setting,
            observation,
        })
        .map(SessionOwnerEvent::Wire)
        .collect(),
        SessionWireRequest::Cancel { .. } => Vec::new(),
    }
}

fn store_replies(request: SessionStoreRequest) -> Vec<SessionOwnerEvent> {
    [
        EffectOutcome::Applied,
        EffectOutcome::Failed,
        EffectOutcome::Unknown,
    ]
    .into_iter()
    .map(|outcome| match &request {
        SessionStoreRequest::ReadRestoreSetup(run) => {
            SessionStoreResult::RestoreSetup { run: *run, outcome }
        }
        SessionStoreRequest::MarkDispatch(input) => SessionStoreResult::Dispatch {
            input: input.clone(),
            outcome,
        },
        SessionStoreRequest::UndoDispatch(input) => SessionStoreResult::UndoDispatch {
            input: input.clone(),
            outcome,
        },
        SessionStoreRequest::MarkSteerDispatch(input) => SessionStoreResult::SteerDispatch {
            input: input.clone(),
            outcome,
        },
        SessionStoreRequest::ReadDispatch(input) => SessionStoreResult::DispatchReadback {
            input: input.clone(),
            outcome,
        },
        SessionStoreRequest::RecordConfirmedSetting { setting, .. } => {
            SessionStoreResult::ConfirmedSetting {
                setting: *setting,
                outcome,
            }
        }
    })
    .map(SessionOwnerEvent::Store)
    .collect()
}

#[hegel::test]
fn generated_mailbox_orders_preserve_the_five_owner_invariants(case: hegel::TestCase) {
    let mut mailbox = Mailbox::new();
    // Reach each oracle even on the smallest shrunk suffix.
    let first = RunAttempt {
        generation: SessionGeneration::from_number(1),
        attempt: LoadAttempt(uuid(1)),
    };
    mailbox.deliver(SessionOwnerEvent::Command(SessionOwnerCommand::Restore {
        attempt: first.attempt,
        via: RestoreVia::Load,
    }));
    mailbox.deliver(SessionOwnerEvent::Store(SessionStoreResult::RestoreSetup {
        run: first,
        outcome: EffectOutcome::Applied,
    }));
    mailbox.deliver(restore_reply(first));
    let first_input = input(first.generation, 2);
    mailbox.deliver(SessionOwnerEvent::Command(SessionOwnerCommand::Prompt {
        input: first_input.clone(),
        admission: PromptAdmission::Direct,
    }));
    mailbox.deliver(mark_prompt(&first_input));
    let closing = RunAttempt {
        generation: first.generation,
        attempt: CloseAttempt(uuid(3)),
    };
    mailbox.deliver(SessionOwnerEvent::Command(SessionOwnerCommand::Close {
        generation: closing.generation,
        attempt: closing.attempt,
    }));
    mailbox.deliver(SessionOwnerEvent::DeadlineElapsed(SessionDeadline::Close(
        closing,
    )));
    // Old replies remain in the mailbox alongside current commands.
    let steps = case.draw(gs::integers::<u8>().min_value(1).max_value(70));
    for number in 10..10 + u64::from(steps) {
        let choice = case.draw(gs::integers::<u8>().max_value(11));
        let generation = match mailbox.owner.lifecycle() {
            SessionLifecycle::NotLoaded { last } => last,
            SessionLifecycle::Live { generation }
            | SessionLifecycle::Loading { generation, .. }
            | SessionLifecycle::Closing { generation, .. } => generation,
        };
        let mut next_input = input(generation, number);
        if let SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } =
            mailbox.owner.turn()
            && choice == 5
        {
            next_input.turn = turn;
        }
        let command = match choice {
            0 => SessionOwnerCommand::Restore {
                attempt: LoadAttempt(uuid(number)),
                via: RestoreVia::Load,
            },
            1 => SessionOwnerCommand::Close {
                generation,
                attempt: CloseAttempt(uuid(number)),
            },
            2..=4 => SessionOwnerCommand::Prompt {
                input: next_input,
                admission: PromptAdmission::Auto {
                    steering_supported: true,
                },
            },
            5 => SessionOwnerCommand::Steer {
                input: next_input,
                supported: true,
            },
            6 => SessionOwnerCommand::Set {
                setting: OwnerSetting {
                    change: RunAttempt {
                        generation,
                        attempt: SettingApplication(uuid(number)),
                    },
                    kind: ProviderSettingKind::Fast,
                },
                value: SettingValueId("off".to_owned()),
            },
            _ => {
                let index = case.draw(
                    gs::integers::<usize>().max_value(mailbox.arrivals.len().saturating_sub(1)),
                );
                if let Some(event) = mailbox.arrivals.get(index).cloned() {
                    mailbox.deliver(event);
                }
                continue;
            }
        };
        mailbox.deliver(SessionOwnerEvent::Command(command));
    }
    // Once new commands stop, drive every currently armed wait from the
    // injected clock. Each input can advance through at most four waits.
    let budget = 4 * (pending(&mailbox.owner).len() + 1);
    for _ in 0..budget {
        let Some(wait) = pending(&mailbox.owner).into_iter().next() else {
            break;
        };
        mailbox.deliver(SessionOwnerEvent::DeadlineElapsed(wait));
    }
    assert!(
        pending(&mailbox.owner).is_empty(),
        "(c) transient work did not drain within the deadline budget"
    );
    assert!(
        !matches!(
            mailbox.owner.lifecycle(),
            SessionLifecycle::Loading { .. } | SessionLifecycle::Closing { .. }
        ),
        "(c) the owner remained in a transient lifecycle"
    );
}
