use super::*;
use crate::session_lifecycle_machine::CloseOrigin;
use hegel::generators as gs;
use std::collections::BTreeSet;

#[derive(Clone)]
struct Mailbox {
    owner: SessionOwnerState,
    arrivals: Vec<OwnerEvent>,
    armed: Vec<OwnerDeadline>,
    cancel_takes: BTreeSet<DispatchAuthorization>,
}

impl Mailbox {
    fn new() -> Self {
        Self {
            owner: fresh(),
            arrivals: Vec::new(),
            armed: Vec::new(),
            cancel_takes: BTreeSet::new(),
        }
    }

    fn deliver(&mut self, event: OwnerEvent) {
        if let OwnerEvent::Dispatch(DispatchResult::CancelTaken { authorization, .. }) = &event {
            self.cancel_takes.insert(authorization.clone());
        }
        let before = self.owner.snapshot().lifecycle;
        let previous_waits = self.pending();
        let fences = sent_settings(&self.owner);
        let restore_fences = self.owner.ports.restore_setting_fences.clone();
        if let Ok(commit) = decide(&self.owner, event.clone()) {
            self.owner = commit.next;
            assert_live_commit(before, self.owner.snapshot().lifecycle, &event);
            for effect in commit.effects.effects {
                self.observe(effect);
            }
        }
        let remaining = self.pending();
        assert!(
            remaining.iter().all(|wait| self.armed.contains(wait)),
            "I3 pending work has no armed deadline: {remaining:?}"
        );
        if let OwnerEvent::Deadline(wait) = &event {
            assert!(
                !previous_waits.contains(wait) || !remaining.contains(wait),
                "I3 expired work stayed pending: {wait:?}"
            );
        }
        assert_fences(fences, &self.owner, &event);
        for (kind, (run, setting)) in restore_fences {
            if self.owner.ports.restore_setting_fences.get(&kind) != Some(&(run, setting)) {
                assert!(
                    matches!(event, OwnerEvent::ConnectionLost)
                        || matches!(event, OwnerEvent::Wire(SessionWireResult::RestoreMode { run: reply_run, setting: reply_setting, .. })
                        | OwnerEvent::Prepare(RestorePreparation::FastOff { run: reply_run, setting: reply_setting, .. })
                        if reply_run == run && reply_setting == setting),
                    "I8 a restore setting lost its fence before exact correlation or connection loss"
                );
            }
        }
        if !matches!(before, SessionLifecycle::NotLoaded { .. })
            && let SessionLifecycle::NotLoaded { last } = self.owner.snapshot().lifecycle
        {
            let recovery = decide(
                &self.owner,
                OwnerEvent::Command(SessionOwnerCommand::Restore {
                    attempt: LoadAttempt(uuid(999)),
                    via: RestoreVia::Load,
                }),
            );
            if self.owner.ports.connection_alive {
                assert!(
                    matches!(recovery, Ok(SessionOwnerCommit { next, .. }) if matches!(
                        next.snapshot().lifecycle, SessionLifecycle::Loading { generation, .. } if generation > last
                    ))
                );
            } else {
                assert!(matches!(recovery, Err(SessionOwnerRejection::Refused(_))));
            }
        }
    }

    fn pending(&self) -> Vec<OwnerDeadline> {
        pending(&self.owner, &self.cancel_takes)
    }

    fn observe(&mut self, effect: OwnerEffect) {
        match effect {
            OwnerEffect::Arm { deadline, budget } => {
                if matches!(deadline, OwnerDeadline::CancelConfirmation(_)) {
                    assert_eq!(budget, std::time::Duration::from_secs(30));
                }
                self.armed.push(deadline.clone());
                self.arrivals.push(OwnerEvent::Deadline(deadline));
            }
            OwnerEffect::Wire(SessionWireRequest::Restore { run, .. }) => self.arrivals.extend([
                restore_reply(run),
                OwnerEvent::Wire(SessionWireResult::Restore {
                    run,
                    outcome: WireOutcome::Confirmed(RestoreLoadReadback {
                        catalogue: Arc::default(),
                        router_auto_requested: false,
                        mode: RestoredMode::Reapply {
                            application: SettingApplication(run.attempt.0),
                            value: SettingValueId("auto".into()),
                        },
                        fast_on: None,
                    }),
                }),
                OwnerEvent::Wire(SessionWireResult::RestoreFailed {
                    run,
                    failure: RestoreFailure::Disconnected,
                }),
            ]),
            OwnerEffect::Wire(SessionWireRequest::RestoreMode { run, setting, .. }) => self
                .arrivals
                .push(OwnerEvent::Wire(SessionWireResult::RestoreMode {
                    run,
                    setting,
                    outcome: RestoreModeOutcome::Confirmed(SettingValueId("auto".into())),
                    catalogue: None,
                })),
            OwnerEffect::Wire(SessionWireRequest::Close(run)) => self.arrivals.extend([
                close_reply(run, WireOutcome::Confirmed(())),
                close_reply(run, WireOutcome::Refused),
                close_reply(run, WireOutcome::Unknown),
            ]),
            OwnerEffect::PrepareHost { run, .. } => self.arrivals.extend([
                host_ready(run),
                OwnerEvent::Prepare(RestorePreparation::Failed {
                    run,
                    stage: PreparationStage::HostSetup,
                }),
            ]),
            OwnerEffect::Wire(SessionWireRequest::RestoreFastOff { run, setting, .. }) => {
                self.arrivals
                    .push(OwnerEvent::Prepare(RestorePreparation::FastOff {
                        run,
                        setting,
                        catalogue: None,
                        observation: SettingEvidence {
                            value: ObservedSetting::Confirmed(SettingValueId("off".into())),
                            cause: None,
                        },
                    }));
            }
            OwnerEffect::PrepareRoute(run) => self
                .arrivals
                .push(OwnerEvent::Prepare(RestorePreparation::RouteReady(run))),
            OwnerEffect::SettingQueued { input, setting } => {
                self.arrivals
                    .push(OwnerEvent::Command(SessionOwnerCommand::CancelSetting {
                        input,
                        setting,
                    }))
            }
            OwnerEffect::Store(request) => self.observe_store(request),
            OwnerEffect::DrainCancel(authorization) => {
                self.arrivals
                    .push(OwnerEvent::Dispatch(DispatchResult::CancelDrainFinished(
                        authorization,
                    )))
            }
            OwnerEffect::TakeCancel {
                authorization,
                expired,
            } => {
                self.arrivals
                    .push(OwnerEvent::Dispatch(DispatchResult::CancelTaken {
                        authorization,
                        drain: if expired {
                            CancelDrainReport::Expired {
                                pending: Vec::new(),
                            }
                        } else {
                            CancelDrainReport::Drained
                        },
                    }));
            }
            OwnerEffect::Authorize {
                authorization,
                work,
            } => {
                self.arrivals
                    .push(OwnerEvent::Dispatch(DispatchResult::Taken(
                        authorization.clone(),
                    )));
                match work {
                    AuthorizedWork::Prompt(_) => {
                        self.arrivals
                            .push(OwnerEvent::Dispatch(DispatchResult::PromptFinished {
                                authorization,
                                outcome: WireOutcome::Confirmed(PromptCompletion {
                                    stop_reason: session_event_model::StopReason::EndTurn,
                                    local_cause: None,
                                    response: String::new(),
                                }),
                            }))
                    }
                    AuthorizedWork::Steer(_) => {
                        self.arrivals
                            .push(OwnerEvent::Dispatch(DispatchResult::SteerFinished {
                                authorization,
                                outcome: WireOutcome::Confirmed(()),
                            }))
                    }
                    AuthorizedWork::Cancel(_) => {}
                    AuthorizedWork::Setting { value, .. } => {
                        self.arrivals
                            .push(OwnerEvent::Dispatch(DispatchResult::SettingFinished {
                                authorization,
                                observation: SettingObservation::Applied(value),
                            }))
                    }
                }
            }
            OwnerEffect::Revoke { authorization, .. } => self
                .arrivals
                .push(OwnerEvent::Dispatch(DispatchResult::Revoked(authorization))),
            OwnerEffect::Publish(
                PublicationBatch::Commit { id, .. } | PublicationBatch::Rebase { id, .. },
            ) => {
                self.arrivals
                    .push(OwnerEvent::Publication(PublicationResult::Applied(id)));
            }
            OwnerEffect::RecordDeliveryEvidence(placement) => {
                self.arrivals.push(OwnerEvent::DeliveryEvidence {
                    placement,
                    outcome: EffectOutcome::Applied,
                });
            }
            OwnerEffect::Wire(_)
            | OwnerEffect::CallbackMembership(_)
            | OwnerEffect::Reply(_)
            | OwnerEffect::ReplyUnsaved(_)
            | OwnerEffect::RestoreSettled { .. }
            | OwnerEffect::AdoptProviderId { .. }
            | OwnerEffect::CreateSettled { .. }
            | OwnerEffect::RetireRoute(_)
            | OwnerEffect::CancelNotSent { .. }
            | OwnerEffect::CancelSent { .. }
            | OwnerEffect::InputQueued(_)
            | OwnerEffect::PromptTaken(_)
            | OwnerEffect::SteerSubmitted(_)
            | OwnerEffect::ProviderStartedTurn(_)
            | OwnerEffect::ReopenRoute(_)
            | OwnerEffect::ForwardTool { .. } => {}
        }
    }

    fn observe_store(&mut self, request: SessionStoreRequest) {
        for outcome in [
            EffectOutcome::Applied,
            EffectOutcome::Failed,
            EffectOutcome::Unknown,
        ] {
            let event = match &request {
                SessionStoreRequest::RecordTerminal { write, .. } => SessionStoreResult::Terminal {
                    write: *write,
                    outcome,
                },
                SessionStoreRequest::ReadRestoreSetup(run) => {
                    SessionStoreResult::RestoreSetup { run: *run, outcome }
                }
                SessionStoreRequest::MarkDispatch(input)
                | SessionStoreRequest::MarkQueuedDispatch(input) => SessionStoreResult::Dispatch {
                    input: input.clone(),
                    outcome,
                },
                SessionStoreRequest::UndoDispatch(input) => SessionStoreResult::UndoDispatch {
                    input: input.clone(),
                    outcome,
                },
                SessionStoreRequest::ReconcileLateCancelledPrompt(input) => {
                    SessionStoreResult::LateCancelledPromptReconciled {
                        input: input.clone(),
                        outcome,
                    }
                }
                SessionStoreRequest::MarkSteerDispatch(input) => {
                    SessionStoreResult::SteerDispatch {
                        input: input.clone(),
                        outcome,
                    }
                }
                SessionStoreRequest::RecordConfirmedSetting { setting, .. } => {
                    SessionStoreResult::ConfirmedSetting {
                        setting: *setting,
                        outcome,
                    }
                }
            };
            self.arrivals.push(OwnerEvent::Store(event));
        }
    }
}

fn assert_live_commit(before: SessionLifecycle, after: SessionLifecycle, event: &OwnerEvent) {
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
                && matches!(event, OwnerEvent::Prepare(
                RestorePreparation::HostReady { run, .. } | RestorePreparation::RouteReady(run)
            ) if *run == expected)
        }
        SessionLifecycle::Closing {
            generation: closing,
            from: CloseOrigin::Live,
            attempt,
            ..
        } => {
            closing == generation
                && matches!(event, OwnerEvent::Wire(SessionWireResult::Close { run, outcome: WireOutcome::Refused })
                if *run == RunAttempt { generation, attempt })
        }
        SessionLifecycle::NotLoaded { last } => {
            generation > last && matches!(event, OwnerEvent::Create(CreateProgress::Adopted { .. }))
        }
        SessionLifecycle::Live { .. } | SessionLifecycle::Closing { .. } => false,
    };
    assert!(
        committed,
        "I1 Live must follow one matching usable-run commit: {before:?}, {event:?}, {after:?}"
    );
}

fn pending(
    state: &SessionOwnerState,
    cancel_takes: &BTreeSet<DispatchAuthorization>,
) -> Vec<OwnerDeadline> {
    let mut waits = Vec::new();
    if let Some(restore) = state.restore {
        waits.push(match restore {
            PendingRestore::Setup { run, .. } => OwnerDeadline::ReadSetup(run),
            PendingRestore::Wire { run, .. } => OwnerDeadline::RestoreWire(run),
            PendingRestore::Mode { run, .. } => OwnerDeadline::RestoreMode(run),
            PendingRestore::FastOff { run, .. } => OwnerDeadline::RestoreFastOff(run),
            PendingRestore::Preparing { run, .. } => OwnerDeadline::HostPrepare(run),
        });
    }
    if let SessionLifecycle::Closing {
        generation,
        attempt,
        ..
    } = state.lifecycle()
    {
        waits.push(OwnerDeadline::Close(RunAttempt {
            generation,
            attempt,
        }));
    }
    for prompt in state.prompts.values().chain(state.steer.iter()) {
        match prompt.wait {
            InputWait::DispatchStore => waits.push(OwnerDeadline::DispatchMark {
                input: prompt.input.clone(),
            }),
            InputWait::UndoStore => waits.push(OwnerDeadline::UndoStore {
                input: prompt.input.clone(),
            }),
            InputWait::Admission | InputWait::Enqueue | InputWait::Wire => {}
        }
    }
    waits.extend(
        state
            .ports
            .unsent_prompts
            .keys()
            .map(|input| OwnerDeadline::UndoStore {
                input: input.clone(),
            }),
    );
    for (key, authorization) in &state.ports.authorizations {
        match (&authorization.status, &authorization.work) {
            // Cancel takes use the drain's budget, with no second enqueue timer.
            (AuthorizationStatus::Offered, AuthorizedWork::Cancel(_)) => {}
            (AuthorizationStatus::Offered, _) => waits.push(OwnerDeadline::Enqueue(key.clone())),
            (AuthorizationStatus::Taken, AuthorizedWork::Steer(_)) => {
                waits.push(OwnerDeadline::SteerResult(key.clone()))
            }
            (AuthorizationStatus::DrainingCancel, _) => {
                waits.push(OwnerDeadline::CancelDrain(key.clone()));
            }
            _ => {}
        }
    }
    waits.extend(
        cancel_takes
            .iter()
            .cloned()
            .map(OwnerDeadline::CancelConfirmation)
            .filter(|deadline| state.has_pending_deadline(deadline)),
    );
    match state.setting {
        Some((setting, SettingWait::Wire)) => waits.push(OwnerDeadline::SettingWire(setting)),
        Some((setting, SettingWait::Store)) => waits.push(OwnerDeadline::SettingStore(setting)),
        _ => {}
    }
    waits.extend(
        state
            .ports
            .awaiting_apply
            .iter()
            .copied()
            .map(OwnerDeadline::Publication),
    );
    waits.extend(state.ports.rebase.map(OwnerDeadline::Rebase));
    waits.extend(state.ports.create.map(OwnerDeadline::Create));
    waits.extend(
        state
            .ports
            .pending_terminal
            .keys()
            .copied()
            .map(OwnerDeadline::TerminalStore),
    );
    waits
}

fn sent_settings(state: &SessionOwnerState) -> BTreeSet<DispatchAuthorization> {
    state
        .ports
        .authorizations
        .iter()
        .filter_map(|(key, pending)| {
            (pending.status == AuthorizationStatus::Taken
                && matches!(pending.work, AuthorizedWork::Setting { .. }))
            .then_some(key.clone())
        })
        .collect()
}

fn assert_fences(
    before: BTreeSet<DispatchAuthorization>,
    after: &SessionOwnerState,
    event: &OwnerEvent,
) {
    let retained = sent_settings(after);
    for key in before.difference(&retained) {
        assert!(
            matches!(event, OwnerEvent::ConnectionLost)
                || matches!(event, OwnerEvent::Dispatch(DispatchResult::SettingFinished { authorization, .. } | DispatchResult::DefinitelyUnsent(authorization)) if authorization == key),
            "I8 a sent setting lost its fence before exact correlation or connection loss: {key:?}"
        );
    }
}

fn exercise_cancel_confirmation(
    mut mailbox: Mailbox,
    case: &hegel::TestCase,
    generation: SessionGeneration,
) {
    let prompt = input(
        generation,
        case.draw(gs::integers::<u64>().min_value(4).max_value(999)),
    );
    mailbox.deliver(OwnerEvent::Command(SessionOwnerCommand::Prompt {
        input: prompt.clone(),
        admission: PromptAdmission::Direct,
    }));
    mailbox.deliver(mark_prompt(&prompt));
    let prompt_token = mailbox
        .arrivals
        .iter()
        .find_map(|event| match event {
            OwnerEvent::Dispatch(DispatchResult::Taken(key)) if key.input_id == prompt.input => {
                Some(key.clone())
            }
            _ => None,
        })
        .expect("wire prompt was authorized");
    mailbox.deliver(OwnerEvent::Dispatch(DispatchResult::Taken(prompt_token)));
    mailbox.deliver(OwnerEvent::Command(SessionOwnerCommand::CancelTurn {
        generation,
        turn: prompt.turn,
    }));
    let cancel = mailbox
        .arrivals
        .iter()
        .find_map(|event| match event {
            OwnerEvent::Dispatch(DispatchResult::CancelDrainFinished(key)) => Some(key.clone()),
            _ => None,
        })
        .expect("cancel has an exact drain authorization");
    assert_eq!(cancel.run, generation);
    assert_eq!(
        cancel.target,
        AuthorizationTarget::ExistingTurn { turn: prompt.turn }
    );
    mailbox.deliver(OwnerEvent::Dispatch(DispatchResult::CancelDrainFinished(
        cancel.clone(),
    )));
    mailbox.deliver(OwnerEvent::Dispatch(DispatchResult::CancelTaken {
        authorization: cancel.clone(),
        drain: CancelDrainReport::Drained,
    }));

    let confirmation = OwnerDeadline::CancelConfirmation(cancel.clone());
    let mut stale = cancel;
    match case.draw(gs::integers::<u8>().max_value(2)) {
        0 => stale.incarnation = OwnerIncarnation(uuid(777)),
        1 => stale.run = SessionGeneration::UNTRACKED,
        _ => {
            stale.target = AuthorizationTarget::ExistingTurn {
                turn: TurnId(uuid(1)),
            }
        }
    }
    let before = mailbox.owner.snapshot();
    assert!(
        mailbox.pending().contains(&confirmation),
        "the exact cancel is waiting for confirmation"
    );
    mailbox.deliver(OwnerEvent::Deadline(OwnerDeadline::CancelConfirmation(
        stale,
    )));
    assert_eq!(
        mailbox.owner.snapshot(),
        before,
        "another cancel cannot expire this turn"
    );

    mailbox.deliver(OwnerEvent::Deadline(confirmation.clone()));
    assert!(!mailbox.pending().contains(&confirmation));
    assert_eq!(
        mailbox.owner.turn(),
        SessionTurn::Cancelling { turn: prompt.turn },
        "an unconfirmed cancel must keep its exact turn fenced"
    );
    assert_eq!(mailbox.owner.snapshot().cancel_unconfirmed, Some(prompt));
}

#[hegel::test(seed = Some(29118), test_cases = 100)]
fn queued_mark_busy_keeps_the_exact_head_and_acknowledgement(case: hegel::TestCase) {
    let mut state = fresh();
    let run = restore_live(&mut state, 1);
    let head = input(run.generation, 71);
    let tail = input(run.generation, 72);
    for queued in [&head, &tail] {
        let effects = command(
            &mut state,
            SessionOwnerCommand::Prompt {
                input: queued.clone(),
                admission: PromptAdmission::Queue,
            },
        );
        assert_eq!(
            effects
                .iter()
                .filter(|effect| **effect == OwnerEffect::InputQueued(queued.clone()))
                .count(),
            1
        );
    }
    let arrivals = case.draw(gs::integers::<u8>().min_value(1).max_value(12));
    for ordinal in 0..arrivals {
        let attempt = uuid(100 + u64::from(ordinal));
        let parked = apply(
            &mut state,
            OwnerEvent::Store(SessionStoreResult::QueuedDispatchBusy {
                input: head.clone(),
                attempt,
            }),
        );
        assert!(
            parked.is_empty(),
            "the head has no mark, settlement or second acknowledgement"
        );
        let snapshots = state.snapshot().queue;
        assert_eq!(
            snapshots
                .iter()
                .map(|entry| &entry.input)
                .collect::<Vec<_>>(),
            vec![&head, &tail]
        );
        assert!(snapshots.iter().all(|entry| !entry.started));
        assert!(!state.has_pending_deadline(&OwnerDeadline::DispatchMark {
            input: head.clone()
        }));
        if case.draw(gs::booleans()) {
            let stale = decide(
                &state,
                OwnerEvent::Store(SessionStoreResult::QueuedDispatchReady {
                    input: head.clone(),
                    attempt: uuid(99),
                }),
            );
            assert!(matches!(stale, Err(SessionOwnerRejection::IgnoredLate)));
        }
        if case.draw(gs::booleans()) {
            let effects = command(
                &mut state,
                SessionOwnerCommand::CancelQueued {
                    generation: head.generation,
                    input: head.input.clone(),
                },
            );
            assert!(
                !effects
                    .iter()
                    .any(|effect| matches!(effect, OwnerEffect::InputQueued(_)))
            );
            assert!(
                !state
                    .snapshot()
                    .queue
                    .iter()
                    .any(|entry| entry.input == head)
            );
            assert!(
                state
                    .snapshot()
                    .queue
                    .iter()
                    .any(|entry| entry.input == tail)
            );
            let stale = decide(
                &state,
                OwnerEvent::Store(SessionStoreResult::QueuedDispatchReady {
                    input: head,
                    attempt,
                }),
            );
            assert!(matches!(stale, Err(SessionOwnerRejection::IgnoredLate)));
            return;
        }
        let ready = apply(
            &mut state,
            OwnerEvent::Store(SessionStoreResult::QueuedDispatchReady {
                input: head.clone(),
                attempt,
            }),
        );
        assert!(ready.contains(&OwnerEffect::Store(
            SessionStoreRequest::MarkQueuedDispatch(head.clone())
        )));
        assert!(!ready.iter().any(|effect| matches!(
            effect,
            OwnerEffect::InputQueued(_)
                | OwnerEffect::Reply(_)
                | OwnerEffect::ReplyUnsaved(_)
                | OwnerEffect::Authorize { .. }
        )));
    }
}

#[hegel::test]
fn generated_orders_check_i1_i3_i7_and_i8(case: hegel::TestCase) {
    let mut mailbox = Mailbox::new();
    let first = RunAttempt {
        generation: SessionGeneration::from_number(1),
        attempt: LoadAttempt(uuid(1)),
    };
    mailbox.deliver(OwnerEvent::Command(SessionOwnerCommand::Restore {
        attempt: first.attempt,
        via: RestoreVia::Load,
    }));
    mailbox.deliver(OwnerEvent::Store(SessionStoreResult::RestoreSetup {
        run: first,
        outcome: EffectOutcome::Applied,
    }));
    mailbox.deliver(restore_reply(first));
    mailbox.deliver(host_ready(first));
    let old_incarnation = OwnerMailboxEvent {
        owner: OwnerAddress {
            incarnation: OwnerIncarnation(uuid(777)),
            ..mailbox.owner.snapshot().owner
        },
        event: OwnerEvent::Prepare(RestorePreparation::RouteReady(first)),
    };
    assert!(
        matches!(
            mailbox.owner.decide(old_incarnation),
            Err(SessionOwnerRejection::IgnoredLate)
        ),
        "I7 old incarnation changed the current attempt"
    );
    mailbox.deliver(OwnerEvent::Prepare(RestorePreparation::RouteReady(first)));
    exercise_cancel_confirmation(mailbox.clone(), &case, first.generation);
    let setting = OwnerSetting {
        change: RunAttempt {
            generation: first.generation,
            attempt: SettingApplication(uuid(2)),
        },
        kind: ProviderSettingKind::Fast,
    };
    mailbox.deliver(OwnerEvent::Command(setting_command(setting, 2)));
    let token = mailbox
        .arrivals
        .iter()
        .rev()
        .find_map(|event| match event {
            OwnerEvent::Dispatch(DispatchResult::Taken(token)) => Some(token.clone()),
            _ => None,
        })
        .expect("authorized setting");
    mailbox.deliver(OwnerEvent::Dispatch(DispatchResult::Taken(token.clone())));
    mailbox.deliver(OwnerEvent::Deadline(OwnerDeadline::SettingWire(setting)));
    mailbox.deliver(OwnerEvent::Dispatch(DispatchResult::SettingFinished {
        authorization: token,
        observation: SettingObservation::Applied(SettingValueId("off".into())),
    }));
    let close = RunAttempt {
        generation: first.generation,
        attempt: CloseAttempt(uuid(3)),
    };
    mailbox.deliver(OwnerEvent::Command(SessionOwnerCommand::Close {
        generation: close.generation,
        attempt: close.attempt,
    }));
    mailbox.deliver(OwnerEvent::Deadline(OwnerDeadline::Close(close)));

    let steps = case.draw(gs::integers::<u8>().min_value(1).max_value(75));
    for number in 20..20 + u64::from(steps) {
        let choice = case.draw(gs::integers::<u8>().max_value(9));
        let generation = mailbox
            .owner
            .cell
            .run_generation()
            .unwrap_or(SessionGeneration::UNTRACKED);
        let command = match choice {
            0 => SessionOwnerCommand::Restore {
                attempt: LoadAttempt(uuid(number)),
                via: RestoreVia::Load,
            },
            1 => SessionOwnerCommand::Close {
                generation,
                attempt: CloseAttempt(uuid(number)),
            },
            2 => SessionOwnerCommand::Prompt {
                input: input(generation, number),
                admission: PromptAdmission::Queue,
            },
            3 => setting_command(
                OwnerSetting {
                    change: RunAttempt {
                        generation,
                        attempt: SettingApplication(uuid(number)),
                    },
                    kind: ProviderSettingKind::Mode,
                },
                number,
            ),
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
        mailbox.deliver(OwnerEvent::Command(command));
    }

    // Stable Live/NotLoaded, ordinary running turns and transport-retained
    // setting fences are not bounded attempts. Publication retries may
    // continue after degradation; each individual offer has its own budget.
    let waits = mailbox.pending();
    for wait in waits {
        mailbox.deliver(OwnerEvent::Deadline(wait));
    }
    if mailbox.pending().is_empty() {
        let before = mailbox.owner.snapshot();
        mailbox.deliver(OwnerEvent::Deadline(OwnerDeadline::Close(close)));
        assert_eq!(
            mailbox.owner.snapshot(),
            before,
            "an old timeout cannot retire healthy stable state"
        );
    }
}
