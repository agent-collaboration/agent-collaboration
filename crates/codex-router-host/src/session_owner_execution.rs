//! Host adapters for one adopted ACP session's owner.

use crate::{
    session_owner_calls::{InputAcceptance, SessionOwnerCalls},
    session_owner_publication::SessionOwnerPublisher,
    session_owner_storage::SessionOwnerStorage,
};
use acp_client_runtime::{
    ExternalProviderRuntimeError,
    acp_transport::TransportRun,
    owner_session_open::{OwnerOpenedSession, OwnerSessionOpenSetup},
    owner_session_setting::{OwnerSettingWirePlan, RestoreSettingPlan, RestoreSettingReadback},
    owner_session_stream::OwnerStreamTarget,
    owner_wire::OwnerAcpWire,
    session_owner_task::{OwnerCompletionPort, OwnerTaskError, SessionOwnerPorts},
};
use collaboration_protocol::{ProviderIdentity, SessionRef};
use provider_session_machines::{
    ProviderSettingKind, SessionGeneration,
    session_lifecycle_machine::{LoadAttempt, RestoreVia},
    session_owner::{
        AuthorizedWork, DispatchAuthorization, DispatchResult, ObservationCause, ObservedSetting,
        OwnerAddress, OwnerEffect, OwnerEvent, OwnerSetting, PreparationStage, PublicationBatch,
        PublicationFact, RestoreFailure, RestoreModeOutcome, RestorePreparation, RunAttempt,
        SessionOwnerReply, SessionPublication, SessionWireRequest, SessionWireResult,
        SettingEvidence, TurnMembership, WireOutcome,
    },
    session_setting_machine::SettingValueId,
};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct SessionOwnerExecution {
    target: SessionRef,
    wire: Arc<OwnerAcpWire>,
    approvals:
        acp_client_runtime::OwnerApprovalBindings<crate::acp_interaction_port::HostInteractionPort>,
    cancel_drains: acp_client_runtime::OwnerCancelDrainExecutor<
        crate::acp_interaction_port::HostInteractionPort,
    >,
    storage: SessionOwnerStorage,
    publisher: SessionOwnerPublisher,
    calls: Arc<SessionOwnerCalls>,
    tasks: TaskTracker,
    opens: Arc<Mutex<HashMap<LoadAttempt, OpenedRestore>>>,
    routes: BTreeMap<SessionGeneration, OwnerRun>,
    created: Option<OwnerOpenedSession>,
    work_roots: crate::provider_work_roots::ProviderWorkRoots,
    #[cfg(test)]
    preparation_fixture: Arc<crate::session_owner_preparation_fixture::PreparationFixture>,
}

struct OpenedRestore {
    session: OwnerOpenedSession,
    fallback: Option<String>,
    skills: Option<session_event_model::SkillView>,
    setting_readback: Option<RestoreSettingReadback>,
}

impl OpenedRestore {
    fn apply_setting_readback(&mut self) {
        if let Some(readback) = self.setting_readback.take() {
            self.session.apply_restore_setting_readback(readback);
        }
    }
}

struct OwnerRun {
    load_attempt: Option<LoadAttempt>,
    cancel: CancellationToken,
    approval: Option<ActiveTurnApproval>,
}

struct ActiveTurnApproval {
    turn: provider_session_machines::session_turn_machine::TurnId,
    approver: ProviderIdentity,
    _lease:
        acp_client_runtime::OwnerApprovalLease<crate::acp_interaction_port::HostInteractionPort>,
}

impl SessionOwnerExecution {
    #[cfg_attr(
        test,
        expect(
            clippy::too_many_arguments,
            reason = "Tests inject the Host preparation seam."
        )
    )]
    pub(crate) fn new(
        target: SessionRef,
        wire: Arc<OwnerAcpWire>,
        approvals: acp_client_runtime::OwnerApprovalBindings<
            crate::acp_interaction_port::HostInteractionPort,
        >,
        storage: SessionOwnerStorage,
        publisher: SessionOwnerPublisher,
        calls: Arc<SessionOwnerCalls>,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
        #[cfg(test)] preparation_fixture: Arc<
            crate::session_owner_preparation_fixture::PreparationFixture,
        >,
    ) -> Self {
        let tasks = storage.task_tracker();
        let cancel_drains = acp_client_runtime::OwnerCancelDrainExecutor::new(approvals.clone());
        Self {
            target,
            wire,
            approvals,
            cancel_drains,
            storage,
            publisher,
            calls,
            tasks,
            work_roots,
            opens: Arc::default(),
            routes: BTreeMap::new(),
            created: None,
            #[cfg(test)]
            preparation_fixture,
        }
    }

    fn session_id(&self) -> String {
        String::from(self.target.session_id.clone())
    }

    fn submitted_input(
        &self,
        input: provider_session_machines::session_owner::OwnerInput,
    ) -> Option<OwnerEvent> {
        let content = self.calls.content(&input.input)?;
        let text = content.submitted_text()?.to_owned();
        Some(OwnerEvent::SubmittedInput {
            input,
            item: session_event_model::SessionItem {
                item_id: uuid::Uuid::now_v7().to_string(),
                kind: session_event_model::SessionItemKind::UserMessage,
                text: Some(text),
            },
        })
    }

    fn transport_run(owner: OwnerAddress, generation: SessionGeneration) -> TransportRun {
        TransportRun {
            incarnation: owner.incarnation,
            generation,
        }
    }

    fn install_approval_for(
        &self,
        owner: OwnerAddress,
        input: &provider_session_machines::session_owner::OwnerInput,
    ) -> Result<Option<ActiveTurnApproval>, acp_client_runtime::ExternalProviderRuntimeError> {
        self.calls
            .take_approval(&input.input)
            .map(|context| {
                let approver = context.approver.clone();
                self.approvals
                    .install(
                        self.session_id(),
                        Self::transport_run(owner, input.generation),
                        input.turn,
                        context,
                    )
                    .map(|lease| ActiveTurnApproval {
                        turn: input.turn,
                        approver,
                        _lease: lease,
                    })
            })
            .transpose()
    }

    fn restore(
        &mut self,
        owner: OwnerAddress,
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let Some(saved) = self.storage.take_setup(run.attempt) else {
            return Ok(Some(restore_failed(
                run,
                RestoreFailure::LocalPrerequisite {
                    stage: PreparationStage::SavedSetup,
                },
            )));
        };
        let completion = completions.reserve(owner)?;
        let cwd = self
            .storage
            .explicit_working_directory(run.attempt)
            .unwrap_or_else(|| PathBuf::from(String::from(saved.working_directory.clone())));
        let restored =
            crate::provider_session_setup::restore_setup(&self.work_roots, &cwd, &saved.resume);
        let setup = OwnerSessionOpenSetup {
            cwd,
            work_roots: restored.work_roots,
            scratch_folder: restored.scratch_folder,
        };
        let route = Self::transport_run(owner, run.generation);
        let provider_id = self.session_id();
        self.wire.open_route(route, provider_id.clone());
        let receipt = match self
            .wire
            .restore(route, run.attempt, provider_id, via, setup)
        {
            Ok(receipt) => receipt,
            Err(_) => return Ok(Some(restore_failed(run, RestoreFailure::NotEnqueued))),
        };
        let paused = CancellationToken::new();
        self.routes.insert(
            run.generation,
            OwnerRun {
                load_attempt: Some(run.attempt),
                cancel: paused.clone(),
                approval: None,
            },
        );
        let opened = Arc::clone(&self.opens);
        let completions = completions.clone();
        self.tasks.spawn(async move {
            let result = tokio::select! {
                () = paused.cancelled() => return,
                result = receipt => result,
            };
            let mut session = match result {
                Ok(Ok(session)) => session,
                Ok(Err(error)) => {
                    completion.send(restore_failed(run, from_wire_failure(error)));
                    return;
                }
                Err(_) => {
                    completion.send(restore_failed(run, RestoreFailure::Disconnected));
                    return;
                }
            };
            let skills = if via == RestoreVia::Load {
                match session.prepare_replay() {
                    Ok(replay) => {
                        if completions
                            .send_stream(
                                owner,
                                OwnerEvent::Replay {
                                    run,
                                    events: replay.events,
                                },
                                &paused,
                            )
                            .await
                            .is_err()
                        {
                            return;
                        }
                        replay.skills
                    }
                    Err(_) => {
                        completion.send(restore_failed(
                            run,
                            RestoreFailure::LocalPrerequisite {
                                stage: PreparationStage::Route,
                            },
                        ));
                        return;
                    }
                }
            } else {
                None
            };
            let RestoreSettingPlan {
                mode,
                accepted_mode_fallback,
                fast_off,
            } = session.restore_setting_plan(saved.resume.requested_mode.as_deref());
            lock(&opened).insert(
                run.attempt,
                OpenedRestore {
                    session,
                    fallback: accepted_mode_fallback,
                    skills,
                    setting_readback: None,
                },
            );
            completion.send(OwnerEvent::Wire(SessionWireResult::Restore {
                run,
                outcome: WireOutcome::Confirmed(mode),
                fast_on: fast_off,
            }));
        });
        Ok(None)
    }

    fn restore_setting(
        &mut self,
        address: OwnerAddress,
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        value: SettingValueId,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let mut connection = lock(&self.opens);
        let Some(opened) = connection.get_mut(&run.attempt) else {
            return Ok(Some(setting_unsent(run, setting)));
        };
        opened.apply_setting_readback();
        let request = match OwnerSettingWirePlan::from_catalog(
            self.session_id(),
            opened.session.catalog(),
            setting.kind,
            value.0,
            opened.fallback.clone(),
        ) {
            Ok(request) => request,
            Err(_) => return Ok(Some(setting_unsent(run, setting))),
        };
        let (request, readback) = request.retain_restore_readback();
        let completion = completions.reserve(address)?;
        if self
            .wire
            .reapply_setting(address.incarnation, run, setting, request, completion)
            .is_err()
        {
            return Ok(Some(setting_unsent(run, setting)));
        }
        opened.setting_readback = Some(readback);
        Ok(None)
    }

    fn authorize(
        &mut self,
        owner: OwnerAddress,
        authorization: DispatchAuthorization,
        work: AuthorizedWork,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let provider_id = self.session_id();
        let setting_operation = match &work {
            AuthorizedWork::Setting { setting, .. } => Some(setting.change.attempt.0),
            _ => None,
        };
        let offered = match work {
            AuthorizedWork::Cancel(_) => return Err(OwnerTaskError::ExecutorStopped),
            AuthorizedWork::Prompt(input) => {
                let content = self.calls.content(&input.input);
                if let Some(content) = content {
                    let completion = completions.reserve(owner)?;
                    let approval = self.install_approval_for(owner, &input);
                    let Ok(approval) = approval else {
                        return Ok(Some(OwnerEvent::Dispatch(
                            DispatchResult::DefinitelyUnsent(authorization),
                        )));
                    };
                    let offered = self
                        .wire
                        .offer_prompt(
                            authorization.clone(),
                            input.turn,
                            provider_id,
                            content,
                            completion,
                        )
                        .is_ok();
                    if offered && let Some(run) = self.routes.get_mut(&input.generation) {
                        run.approval = approval;
                    }
                    offered
                } else {
                    false
                }
            }
            AuthorizedWork::Steer(input) => {
                let approver = self
                    .routes
                    .get(&input.generation)
                    .and_then(|run| run.approval.as_ref())
                    .filter(|approval| approval.turn == input.turn)
                    .map(|approval| &approval.approver);
                self.calls.capture_steer_target(&input, approver);
                let content = self.calls.content(&input.input);
                if let Some(content) = content {
                    let completion = completions.reserve(owner)?;
                    self.wire
                        .offer_steer(authorization.clone(), provider_id, content, completion)
                        .is_ok()
                } else {
                    false
                }
            }
            AuthorizedWork::Setting { setting, value, .. } => {
                if !self.calls.settlement_requested(setting.change.attempt.0) {
                    return Ok(Some(OwnerEvent::Dispatch(
                        DispatchResult::DefinitelyUnsent(authorization),
                    )));
                }
                match self.wire.offer_explicit_setting(
                    authorization.clone(),
                    provider_id,
                    setting.kind,
                    value.0,
                    completions.reserve(owner)?,
                ) {
                    Ok(Some(outcome)) => return Ok(Some(OwnerEvent::Dispatch(outcome))),
                    Ok(None) => true,
                    Err(_) => false,
                }
            }
        };
        if !offered {
            return Ok(Some(OwnerEvent::Dispatch(
                DispatchResult::DefinitelyUnsent(authorization),
            )));
        }
        Ok(self
            .wire
            .take_when(&authorization, || {
                setting_operation.is_none_or(|operation| self.calls.settlement_requested(operation))
            })
            .ok()
            .map(OwnerEvent::Dispatch))
    }

    fn activate_reader(
        &mut self,
        owner: OwnerAddress,
        run: RunAttempt<LoadAttempt>,
        completions: &OwnerCompletionPort,
    ) {
        let Some(opened) = lock(&self.opens).remove(&run.attempt) else {
            return;
        };
        self.attach_stream(
            owner,
            run.generation,
            opened.session,
            opened.skills,
            completions,
        );
    }

    pub(crate) fn prepare_created(&mut self, session: OwnerOpenedSession) {
        self.created = Some(session);
    }

    fn activate_created(
        &mut self,
        owner: OwnerAddress,
        generation: SessionGeneration,
        completions: &OwnerCompletionPort,
    ) {
        let Some(session) = self.created.take() else {
            return;
        };
        self.wire
            .open_route(Self::transport_run(owner, generation), self.session_id());
        self.routes.insert(
            generation,
            OwnerRun {
                load_attempt: None,
                cancel: CancellationToken::new(),
                approval: None,
            },
        );
        self.attach_stream(owner, generation, session, None, completions);
    }

    fn attach_stream(
        &mut self,
        owner: OwnerAddress,
        generation: SessionGeneration,
        mut session: OwnerOpenedSession,
        skills: Option<session_event_model::SkillView>,
        completions: &OwnerCompletionPort,
    ) {
        let route = Self::transport_run(owner, generation);
        self.wire.activate_route(route);
        let Some(held) = self.routes.get_mut(&generation) else {
            return;
        };
        self.wire.install_catalog(route, session.catalog().clone());
        session.register_in_folder_scope(&self.wire, route);
        let retired = held.cancel.clone();
        let completion = completions.clone();
        let wire = Arc::clone(&self.wire);
        let stream =
            OwnerStreamTarget::new(owner, route, wire, completion.clone(), retired.clone());
        self.tasks.spawn(async move {
            if let Some(view) = skills {
                let _owner_retired = completion
                    .send_stream(
                        owner,
                        OwnerEvent::SkillView {
                            run: generation,
                            view,
                        },
                        &retired,
                    )
                    .await;
            }
            if session.read_stream(stream).await.is_err() {
                let _owner_retired = completion
                    .send_stream(owner, OwnerEvent::RunLost(generation), &retired)
                    .await;
            }
        });
    }
}

impl SessionOwnerPorts for SessionOwnerExecution {
    fn execute(
        &mut self,
        owner: OwnerAddress,
        effect: OwnerEffect,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        match effect {
            OwnerEffect::RecordDeliveryEvidence(placement) => {
                self.calls.offer_delivery_evidence(
                    placement,
                    completions.reserve(owner)?,
                    &self.tasks,
                );
                Ok(None)
            }
            OwnerEffect::Store(request) => self.storage.execute(owner, request, completions),
            OwnerEffect::Wire(SessionWireRequest::Restore { run, via }) => {
                self.restore(owner, run, via, completions)
            }
            OwnerEffect::Wire(SessionWireRequest::RestoreMode {
                run,
                setting,
                value,
            }) => self.restore_setting(owner, run, setting, value, completions),
            OwnerEffect::Wire(SessionWireRequest::RestoreFastOff { run, setting }) => self
                .restore_setting(
                    owner,
                    run,
                    setting,
                    SettingValueId("off".into()),
                    completions,
                ),
            OwnerEffect::Wire(SessionWireRequest::Close(run)) => {
                let completion = completions.reserve(owner)?;
                let unavailable = self
                    .wire
                    .enqueue_close(owner.incarnation, run, self.session_id(), completion)
                    .is_err();
                Ok(unavailable.then_some(OwnerEvent::Wire(SessionWireResult::CloseNotSent(run))))
            }
            OwnerEffect::Wire(SessionWireRequest::Create { .. }) => {
                Err(OwnerTaskError::ExecutorStopped)
            }
            OwnerEffect::PrepareRoute(run) => Ok(Some(OwnerEvent::Prepare(
                if lock(&self.opens).contains_key(&run.attempt) {
                    RestorePreparation::RouteReady(run)
                } else {
                    RestorePreparation::Failed {
                        run,
                        stage: PreparationStage::Route,
                    }
                },
            ))),
            OwnerEffect::PrepareHost(run) => {
                let mut openings = lock(&self.opens);
                let preparation = match openings.get_mut(&run.attempt) {
                    Some(entry) => {
                        entry.apply_setting_readback();
                        RestorePreparation::HostReady {
                            run,
                            capabilities: entry.session.capabilities().to_session_model(),
                            settings: entry.session.settings(),
                            observed:
                                acp_client_runtime::owner_session_creation::catalogue_evidence(
                                    entry.session.catalog(),
                                ),
                        }
                    }
                    None => RestorePreparation::Failed {
                        run,
                        stage: PreparationStage::HostSetup,
                    },
                };
                #[cfg(test)]
                {
                    let failed = move || RestorePreparation::Failed {
                        run,
                        stage: PreparationStage::HostSetup,
                    };
                    let fixture = self.preparation_fixture.clone();
                    if let Some(release) = fixture.hold_restore() {
                        let completion = completions.reserve(owner)?;
                        self.tasks.spawn(async move {
                            let _released = release.await;
                            let result = if fixture.take_restore_fault() {
                                failed()
                            } else {
                                preparation
                            };
                            completion.send(OwnerEvent::Prepare(result));
                        });
                        return Ok(None);
                    }
                    if fixture.take_restore_fault() {
                        return Ok(Some(OwnerEvent::Prepare(failed())));
                    }
                }
                Ok(Some(OwnerEvent::Prepare(preparation)))
            }
            OwnerEffect::Authorize {
                authorization,
                work,
            } => self.authorize(owner, authorization, work, completions),
            OwnerEffect::DrainCancel(authorization) => {
                let stop = self
                    .routes
                    .get(&authorization.run)
                    .map(|route| route.cancel.clone())
                    .unwrap_or_default();
                let session = self.session_id();
                self.cancel_drains.begin(
                    &session,
                    authorization,
                    owner,
                    completions,
                    &self.tasks,
                    stop,
                )
            }
            OwnerEffect::TakeCancel {
                authorization,
                expired,
            } => {
                let session = self.session_id();
                let result = self
                    .cancel_drains
                    .take(&self.wire, session, authorization, expired);
                Ok(Some(OwnerEvent::Dispatch(result)))
            }
            OwnerEffect::Revoke { authorization, .. } => {
                self.cancel_drains.discard(&authorization);
                let result = self
                    .wire
                    .revoke(&authorization)
                    .unwrap_or(DispatchResult::Revoked(authorization));
                Ok(Some(OwnerEvent::Dispatch(result)))
            }
            OwnerEffect::CallbackMembership(change) => {
                self.wire.apply_membership(owner.incarnation, change);
                if let TurnMembership::End { turn, .. } = change {
                    self.calls.end_turn(turn);
                }
                if let TurnMembership::End { run, turn }
                | TurnMembership::RemoveOnRevoke { run, turn } = change
                    && let Some(runtime) = self.routes.get_mut(&run)
                    && runtime
                        .approval
                        .as_ref()
                        .is_some_and(|active| active.turn == turn)
                {
                    runtime.approval = None;
                }
                Ok(None)
            }
            OwnerEffect::Publish(batch) => {
                if let PublicationBatch::Commit { facts, .. } = &batch {
                    for fact in facts {
                        match fact {
                            PublicationFact::Live(SessionPublication::Activated {
                                run, ..
                            }) => {
                                self.activate_reader(owner, *run, completions);
                            }
                            PublicationFact::Live(SessionPublication::Created(run)) => {
                                self.activate_created(owner, *run, completions);
                            }
                            _ => {}
                        }
                    }
                }
                self.publisher.publish(owner, batch, completions)
            }
            OwnerEffect::Reply(reply) => {
                if let SessionOwnerReply::SettingFinished { setting, .. }
                | SessionOwnerReply::SettingNotSent { setting, .. }
                | SessionOwnerReply::ModelFastFinished { setting, .. } = &reply
                {
                    self.storage.forget_setting(*setting);
                }
                self.calls.settle(reply, true);
                Ok(None)
            }
            OwnerEffect::ReplyUnsaved(reply) => {
                self.calls.settle(reply, false);
                Ok(None)
            }
            OwnerEffect::RestoreSettled { operation, result } => {
                self.calls.deliver_restore(operation, result);
                Ok(None)
            }
            OwnerEffect::RetireRoute(run) => {
                if let Some(route) = self.routes.remove(&run) {
                    route.cancel.cancel();
                    if let Some(attempt) = route.load_attempt {
                        lock(&self.opens).remove(&attempt);
                    }
                }
                self.wire
                    .apply_membership(owner.incarnation, TurnMembership::Retire(run));
                Ok(None)
            }
            OwnerEffect::ReopenRoute(run) => {
                self.wire
                    .reopen_route(Self::transport_run(owner, run.generation), run.attempt);
                Ok(None)
            }
            OwnerEffect::InputQueued(input) => {
                self.calls
                    .accepted(input.operation, InputAcceptance::Queued);
                Ok(None)
            }
            OwnerEffect::SettingQueued { .. } => {
                #[cfg(any(test, feature = "test-support"))]
                self.storage.notice_queued_setting();
                Ok(None)
            }
            OwnerEffect::PromptTaken(input) => {
                self.calls.prompt_taken(input.clone());
                Ok(self.submitted_input(input))
            }
            OwnerEffect::SteerSubmitted(input) => Ok(self.submitted_input(input)),
            OwnerEffect::CancelSent {
                authorization,
                operation,
                drain,
            } => {
                use provider_session_machines::session_owner::CancelDrainReport;
                if let CancelDrainReport::Expired { pending } = &drain {
                    tracing::warn!(
                        session = %self.session_id(),
                        %authorization.authorization_seq,
                        ?pending,
                        "{}", CancelDrainReport::EXPIRED_SUMMARY
                    );
                }
                self.calls
                    .settle_cancel(&authorization, operation, Ok(drain));
                Ok(None)
            }
            OwnerEffect::CancelNotSent {
                authorization,
                operation,
                reason,
            } => {
                self.calls
                    .settle_cancel(&authorization, operation, Err(reason));
                Ok(None)
            }
            OwnerEffect::ProviderStartedTurn(input) => {
                self.calls.record_active_turn(&input);
                if let Some(approval) = self.install_approval_for(owner, &input).ok().flatten()
                    && let Some(run) = self.routes.get_mut(&input.generation)
                {
                    run.approval = Some(approval);
                }
                tracing::debug!(turn = %input.turn.0, "started by a steer; its end isn't reported by the harness until it finishes");
                Ok(self.submitted_input(input))
            }
            OwnerEffect::Arm { .. }
            | OwnerEffect::AdoptProviderId { .. }
            | OwnerEffect::CreateSettled { .. }
            | OwnerEffect::ForwardTool { .. } => Ok(None),
        }
    }
}

impl Drop for SessionOwnerExecution {
    fn drop(&mut self) {
        for route in self.routes.values() {
            route.cancel.cancel();
        }
    }
}

fn from_wire_failure(error: ExternalProviderRuntimeError) -> RestoreFailure {
    if let Some(coded) = error.coded_wire_failure() {
        return RestoreFailure::from_coded(coded);
    }
    match error {
        ExternalProviderRuntimeError::RestoreNoAnswer => RestoreFailure::NoAnswer,
        _ => RestoreFailure::Disconnected,
    }
}

fn restore_failed(run: RunAttempt<LoadAttempt>, failure: RestoreFailure) -> OwnerEvent {
    OwnerEvent::Wire(SessionWireResult::RestoreFailed { run, failure })
}

fn setting_unsent(run: RunAttempt<LoadAttempt>, setting: OwnerSetting) -> OwnerEvent {
    if setting.kind == ProviderSettingKind::Fast {
        OwnerEvent::Prepare(RestorePreparation::FastOff {
            run,
            setting,
            observation: SettingEvidence {
                value: ObservedSetting::Unknown,
                cause: Some(ObservationCause::Unconfirmed),
            },
        })
    } else {
        OwnerEvent::Wire(SessionWireResult::RestoreMode {
            run,
            setting,
            outcome: RestoreModeOutcome::Unconfirmed,
        })
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
