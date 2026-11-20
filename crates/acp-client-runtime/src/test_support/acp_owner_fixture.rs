//! Scripted ACP fixtures addressed through the session owner and ordered wire ports.

use crate::{
    AgentSessionClient, EffectiveProviderSettings, ExternalProviderCreatedSession,
    ExternalProviderLaunch, ExternalProviderRuntimeError, InteractionPort, ProviderBehavior,
    ProviderCapabilityReport, ProviderModelPicker, ProviderPromptContent,
    ProviderPromptDispatchObservation, ProviderSettingRequest, ProviderSettingsCatalog,
    ProviderTurnInput, RequestedProviderSettings, SessionEventSink, SessionRestoreSetup,
    SessionWorkRoots,
    owner_session_open::{OwnerOpenedSession, OwnerSessionOpenSetup},
    session_owner_task::{SessionOwnerHandle, SessionOwnerTask},
};
use provider_session_machines::{
    SessionGeneration,
    session_lifecycle_machine::{CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle},
    session_owner::{
        CancelDrainReport, CloseOutcome, CloseRequest, ConnectionEpoch, CreatedSessionFacts,
        OwnerAddress, OwnerIncarnation, OwnerInput, OwnerSetting, PromptAdmission, RemoteEffect,
        RestoreDisposition, RestoreFailure, RestoreResult, RevokeReason, RunAttempt,
        SessionOwnerCommand, SessionOwnerReply,
    },
    session_setting_machine::{SettingApplication, SettingValueId},
    session_turn_machine::{SessionTurn, TurnId},
};
use router_clock::RuntimeClock;
use session_event_model::InputId;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    ops::Deref,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "acp_owner_fixture_ports.rs"]
mod ports;

type FixtureResult<T> = Result<T, ExternalProviderRuntimeError>;

pub struct FixtureClient<P: InteractionPort> {
    connection: AgentSessionClient<P>,
    sink: Arc<dyn SessionEventSink>,
    epoch: ConnectionEpoch,
    sessions: Mutex<HashMap<String, Arc<FixtureSession<P>>>>,
}

struct FixtureSession<P: InteractionPort> {
    address: OwnerAddress,
    owner: SessionOwnerHandle,
    state: Arc<Mutex<FixtureState<P>>>,
    stop: CancellationToken,
}

struct FixtureState<P: InteractionPort> {
    capabilities: ProviderCapabilityReport,
    replies: HashMap<Uuid, oneshot::Sender<SessionOwnerReply>>,
    inputs: HashMap<InputId, FixtureInput<P>>,
    restores: HashMap<LoadAttempt, (PathBuf, SessionRestoreSetup)>,
    active_context: Option<(TurnId, P::Context)>,
    cancel_report: tokio::sync::watch::Sender<Option<Result<CancelDrainReport, RevokeReason>>>,
}

struct FixtureInput<P: InteractionPort> {
    content: ProviderPromptContent,
    context: Option<P::Context>,
    dispatch: Option<oneshot::Sender<ProviderPromptDispatchObservation>>,
}

impl<P: InteractionPort> Default for FixtureState<P> {
    fn default() -> Self {
        Self {
            capabilities: ProviderCapabilityReport::default(),
            replies: HashMap::new(),
            inputs: HashMap::new(),
            restores: HashMap::new(),
            active_context: None,
            cancel_report: tokio::sync::watch::channel(None).0,
        }
    }
}

impl<P: InteractionPort> Deref for FixtureClient<P> {
    type Target = AgentSessionClient<P>;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl<P: InteractionPort> std::fmt::Debug for FixtureClient<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FixtureClient")
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl<P: InteractionPort> FixtureClient<P> {
    fn from_connection(connection: AgentSessionClient<P>, sink: Arc<dyn SessionEventSink>) -> Self {
        Self {
            connection,
            sink,
            epoch: ConnectionEpoch(Uuid::now_v7()),
            sessions: Mutex::default(),
        }
    }

    pub async fn initialize(
        launch: ExternalProviderLaunch,
        interactions: Arc<P>,
        sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn ProviderBehavior>,
    ) -> FixtureResult<Self> {
        let connection =
            AgentSessionClient::initialize(launch, interactions, sink.clone(), behavior).await?;
        Ok(Self::from_connection(connection, sink))
    }

    #[cfg(feature = "test-observation")]
    pub async fn initialize_with_model_picker(
        launch: ExternalProviderLaunch,
        picker: ProviderModelPicker,
        interactions: Arc<P>,
        sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn ProviderBehavior>,
    ) -> FixtureResult<Self> {
        let connection = AgentSessionClient::initialize_with_model_picker(
            launch,
            picker,
            interactions,
            sink.clone(),
            behavior,
        )
        .await?;
        Ok(Self::from_connection(connection, sink))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Matches the client constructor used by the fixture cases."
    )]
    pub async fn initialize_with_mcp_http(
        launch: ExternalProviderLaunch,
        picker: ProviderModelPicker,
        name: impl Into<String>,
        url: impl Into<String>,
        interactions: Arc<P>,
        sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn ProviderBehavior>,
        clock: Arc<dyn RuntimeClock>,
    ) -> FixtureResult<Self> {
        let connection = AgentSessionClient::initialize_with_mcp_http(
            launch,
            picker,
            name,
            url,
            interactions,
            sink.clone(),
            behavior,
            clock,
        )
        .await?;
        Ok(Self::from_connection(connection, sink))
    }

    pub async fn initialize_in_process(
        io: tokio::io::DuplexStream,
        agent_task: tokio::task::JoinHandle<()>,
        interactions: Arc<P>,
        sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn ProviderBehavior>,
        clock: Arc<dyn RuntimeClock>,
    ) -> FixtureResult<Self> {
        let connection = AgentSessionClient::initialize_in_process(
            io,
            agent_task,
            interactions,
            sink.clone(),
            behavior,
            clock,
        )
        .await?;
        Ok(Self::from_connection(connection, sink))
    }

    fn session(&self, id: &str) -> FixtureResult<Arc<FixtureSession<P>>> {
        lock(&self.sessions)
            .get(id)
            .cloned()
            .ok_or(ExternalProviderRuntimeError::LocalNotFound)
    }

    fn open_owner(
        &self,
        id: String,
        address: OwnerAddress,
        created: Option<(OwnerOpenedSession, CreatedSessionFacts)>,
    ) -> FixtureResult<Arc<FixtureSession<P>>> {
        use std::collections::hash_map::Entry;
        let mut sessions = lock(&self.sessions);
        match sessions.entry(id.clone()) {
            Entry::Occupied(_) if created.is_some() => Err(
                ExternalProviderRuntimeError::Operation("duplicate provider session ID".into()),
            ),
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => {
                let capacity = NonZeroUsize::new(32).unwrap_or(NonZeroUsize::MIN);
                let (task, owner) = SessionOwnerTask::new(
                    address,
                    self.connection.runtime_clock(),
                    capacity,
                    capacity,
                );
                let stop = CancellationToken::new();
                let session = Arc::new(FixtureSession {
                    address,
                    owner,
                    state: Arc::default(),
                    stop: stop.clone(),
                });
                let ports = ports::FixturePorts::new(
                    id,
                    session.state.clone(),
                    self.connection.owner_wire(),
                    self.connection.owner_approval_bindings(),
                    self.sink.clone(),
                    stop.clone(),
                    created,
                );
                let connection_retired = self.connection.retirement();
                let completion = session.owner.completion_port();
                let stop_watcher = stop.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        () = stop_watcher.cancelled() => {}
                        () = connection_retired.cancelled() => {
                            let _stopped = completion.send_stream(
                                address,
                                provider_session_machines::session_owner::OwnerEvent::ConnectionLost,
                                &stop_watcher,
                            ).await;
                        }
                    }
                });
                tokio::spawn(task.run(ports, stop));
                entry.insert(session.clone());
                Ok(session)
            }
        }
    }

    fn fresh_address(&self) -> OwnerAddress {
        OwnerAddress {
            incarnation: OwnerIncarnation(Uuid::now_v7()),
            connection_epoch: self.epoch,
        }
    }

    pub async fn create_session(&self, cwd: PathBuf) -> FixtureResult<String> {
        self.create_session_with_settings(cwd, None, RequestedProviderSettings::default())
            .await
            .map(|created| created.provider_session_id)
    }

    pub async fn create_session_with_settings(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        requested: RequestedProviderSettings,
    ) -> FixtureResult<ExternalProviderCreatedSession> {
        self.create_session_with_work_roots(
            cwd,
            scratch_folder,
            requested,
            SessionWorkRoots::default(),
        )
        .await
    }

    pub async fn create_session_with_work_roots(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        requested: RequestedProviderSettings,
        work_roots: SessionWorkRoots,
    ) -> FixtureResult<ExternalProviderCreatedSession> {
        let address = self.fresh_address();
        let mut opened = self
            .owner_wire()
            .create(
                address.incarnation,
                OwnerSessionOpenSetup {
                    cwd,
                    scratch_folder,
                    work_roots,
                },
            )
            .map_err(fixture_error)?
            .await
            .map_err(fixture_error)??;
        let setup = opened
            .configure_create(&requested, self.runtime_clock().as_ref())
            .await?;
        let facts = setup.owner_facts(&opened);
        let session = self.open_owner(opened.provider_id(), address, Some((opened, facts)))?;
        session
            .admit(SessionOwnerCommand::Create {
                operation: Uuid::now_v7(),
            })
            .await?;
        session
            .owner
            .snapshots()
            .wait_for(|snapshot| matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }))
            .await
            .map_err(fixture_error)?;
        if let Some(failure) = setup.settings_failure {
            return Err(failure);
        }
        Ok(setup.created)
    }

    pub async fn load_session(&self, id: String, cwd: PathBuf) -> FixtureResult<()> {
        self.restore(id, cwd, SessionRestoreSetup::default(), RestoreVia::Load)
            .await
            .map(|_| ())
    }

    pub async fn resume_session(&self, id: String, cwd: PathBuf) -> FixtureResult<()> {
        self.restore(id, cwd, SessionRestoreSetup::default(), RestoreVia::Resume)
            .await
            .map(|_| ())
    }

    pub async fn restore(
        &self,
        id: String,
        cwd: PathBuf,
        setup: SessionRestoreSetup,
        via: RestoreVia,
    ) -> FixtureResult<RestoreResult> {
        let capabilities = self.connection.connection_capability_report().await;
        let unsupported = match via {
            RestoreVia::Load if !capabilities.supports_load => Some("session/load"),
            RestoreVia::Resume if !capabilities.supports_resume => Some("session/resume"),
            _ => None,
        };
        if let Some(capability) = unsupported {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability { capability });
        }
        let session = self.open_owner(id, self.fresh_address(), None)?;
        let operation = Uuid::now_v7();
        let attempt = LoadAttempt(operation);
        lock(&session.state).restores.insert(attempt, (cwd, setup));
        match session
            .settle(operation, SessionOwnerCommand::Restore { attempt, via })
            .await?
        {
            SessionOwnerReply::RestoreFinished(result) => match &result.disposition {
                RestoreDisposition::Restored { .. } => Ok(result),
                RestoreDisposition::Retired {
                    failure: RestoreFailure::Closed,
                    ..
                } => Err(ExternalProviderRuntimeError::SessionReleased),
                RestoreDisposition::Retired {
                    failure: RestoreFailure::NoAnswer,
                    ..
                } => Err(ExternalProviderRuntimeError::RestoreNoAnswer),
                RestoreDisposition::Retired { failure, .. }
                    if let Some(coded) = failure.coded_evidence() =>
                {
                    Err(coded.into())
                }
                RestoreDisposition::Retired { failure, .. } => {
                    Err(fixture_error(failure.next_step()))
                }
            },
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn close_session(&self, id: String) -> FixtureResult<()> {
        if !self.capability_report(&id).await.supports_close {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/close",
            });
        }
        let session = self.session(&id)?;
        let generation = session.generation()?;
        let operation = Uuid::now_v7();
        let provider_supports_close = lock(&session.state).capabilities.supports_close;
        let command = SessionOwnerCommand::CloseWithCapability(CloseRequest {
            run: RunAttempt {
                generation,
                attempt: CloseAttempt(operation),
            },
            provider_supports_close,
        });
        match session.settle(operation, command).await? {
            SessionOwnerReply::CloseFinished {
                outcome: CloseOutcome::Confirmed | CloseOutcome::RouterOnly,
                ..
            } => Ok(()),
            SessionOwnerReply::CloseFinished { outcome, .. } => {
                Err(fixture_error(outcome.human_phrase()))
            }
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn set_setting(
        &self,
        id: String,
        request: ProviderSettingRequest,
    ) -> FixtureResult<EffectiveProviderSettings> {
        let session = self.session(&id)?;
        if request.intent == session_event_model::SettingApplicationIntent::ExplicitChange
            && !(request.kind == provider_session_machines::ProviderSettingKind::Mode
                && request.value == crate::AUTO_PERMISSION_MODE)
            && let Some(advertised) = self
                .settings_catalog(&id)
                .and_then(|catalog| catalog.unavailable_selection(request.kind, &request.value))
        {
            return Err(ExternalProviderRuntimeError::InvalidSetting {
                setting: request.kind,
                value: request.value,
                advertised,
                provider_session_id: id,
                disposition: crate::InvalidSettingSessionDisposition::RemainsCreated,
            });
        }
        let operation = Uuid::now_v7();
        let setting = OwnerSetting {
            change: RunAttempt {
                generation: session.generation()?,
                attempt: SettingApplication(operation),
            },
            kind: request.kind,
        };
        let input = InputId::generate();
        let value = SettingValueId(request.value.clone());
        let command = match request.intent {
            session_event_model::SettingApplicationIntent::ExplicitChange => {
                SessionOwnerCommand::Set {
                    input,
                    setting,
                    value,
                    expected_model: request.expected_model.map(SettingValueId),
                }
            }
            session_event_model::SettingApplicationIntent::TurnContext => {
                SessionOwnerCommand::SetInContext {
                    input,
                    setting,
                    value,
                    expected_model: request.expected_model.map(SettingValueId),
                    reuse_confirmed: self.connection.can_reuse_setting_confirmation(request.kind),
                }
            }
        };
        match session.settle(operation, command).await? {
            SessionOwnerReply::SettingKept { .. } => Ok(session.effective_settings()),
            SessionOwnerReply::SettingFinished {
                remote: RemoteEffect::Applied,
                ..
            } => Ok(session.effective_settings()),
            SessionOwnerReply::SettingFinished {
                remote: RemoteEffect::None,
                ..
            } if request.kind == provider_session_machines::ProviderSettingKind::Fast
                && request.value == "auto" =>
            {
                Ok(session.effective_settings())
            }
            SessionOwnerReply::SettingFinished {
                remote: RemoteEffect::Unknown,
                ..
            } => Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                provider_session_id: id,
                setting: request.kind,
                value: request.value,
            }),
            SessionOwnerReply::SettingFinished { .. } => {
                Err(ExternalProviderRuntimeError::SettingFailed {
                    setting: request.kind,
                    value: request.value,
                    reason: "setting was not applied".into(),
                })
            }
            SessionOwnerReply::ModelFastFinished { fast, .. } => {
                use provider_session_machines::{
                    ProviderSettingKind, session_owner::ModelFastOutcome,
                };
                match fast {
                    ModelFastOutcome::ConfirmedOff => Ok(session.effective_settings()),
                    ModelFastOutcome::NotOffered => {
                        Err(ExternalProviderRuntimeError::FastNotOffered)
                    }
                    ModelFastOutcome::Refused => Err(ExternalProviderRuntimeError::SettingFailed {
                        setting: ProviderSettingKind::Fast,
                        value: "off".to_owned(),
                        reason: "the provider refused fast off".to_owned(),
                    }),
                    ModelFastOutcome::Unconfirmed => {
                        Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                            provider_session_id: id,
                            setting: ProviderSettingKind::Fast,
                            value: "off".to_owned(),
                        })
                    }
                }
            }
            SessionOwnerReply::SettingNotSent { refusal, .. } => {
                use provider_session_machines::session_owner::SessionOwnerRefusal;
                Err(match refusal {
                    SessionOwnerRefusal::SettingContextChanged => {
                        ExternalProviderRuntimeError::SettingsContextChanged
                    }
                    SessionOwnerRefusal::SettingsUnresolved => {
                        ExternalProviderRuntimeError::SettingsUnresolved {
                            answer: RevokeReason::SettingsUnresolved.human_phrase().to_owned(),
                        }
                    }
                    other => ExternalProviderRuntimeError::LocalBusy {
                        answer: format!("{other:?}"),
                    },
                })
            }
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn submit_with_approval_dispatch_for_input(
        &self,
        id: String,
        input_id: InputId,
        request: ProviderTurnInput,
        approval: P::Context,
        dispatch: Option<oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> FixtureResult<crate::ExternalProviderPromptOutcome> {
        let approval_operation = P::operation_id(&approval);
        let session = self.session(&id)?;
        let capabilities = lock(&session.state).capabilities.clone();
        let content = match self.prepare_owner_prompt(request, &capabilities) {
            Ok(content) => content,
            Err(error) => {
                report_prompt_refusal(dispatch, &error);
                return Err(error);
            }
        };
        let operation = Uuid::now_v7();
        let input = OwnerInput {
            generation: session.generation()?,
            operation,
            input: input_id.clone(),
            turn: TurnId(Uuid::now_v7()),
        };
        let admission = if matches!(
            session.owner.snapshots().borrow().lifecycle,
            SessionLifecycle::Loading { .. }
        ) {
            PromptAdmission::Auto {
                steering_supported: false,
            }
        } else {
            PromptAdmission::Direct
        };
        let command = if content.selected_skills().is_empty() {
            SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission,
            }
        } else {
            SessionOwnerCommand::PromptWithSkills {
                input: input.clone(),
                admission,
                selected: content.selected_skills().to_vec(),
            }
        };
        lock(&session.state).inputs.insert(
            input.input.clone(),
            FixtureInput {
                content,
                context: Some(approval),
                dispatch,
            },
        );
        let reply = match session.settle(operation, command).await {
            Ok(reply) => reply,
            Err(error) => {
                let dispatch = lock(&session.state)
                    .inputs
                    .remove(&input_id)
                    .and_then(|payload| payload.dispatch);
                report_prompt_refusal(dispatch, &error);
                return Err(error);
            }
        };
        match reply {
            SessionOwnerReply::PromptCompleted { ref completion, .. }
                if completion.local_cause
                    == Some(session_event_model::LocalCause::OutputOverflow) =>
            {
                Err(ExternalProviderRuntimeError::PromptOutputLimitExceeded)
            }
            SessionOwnerReply::PromptCompleted { ref completion, .. }
                if matches!(completion.local_cause.as_ref(),
                    Some(session_event_model::LocalCause::Other(cause)) if cause == "compactionUnconfirmed") =>
            {
                Err(ExternalProviderRuntimeError::CompactionUnconfirmed)
            }
            SessionOwnerReply::PromptFailed { failure, .. } => Err(failure.into()),
            SessionOwnerReply::PromptCompleted { completion, .. } => {
                Ok(crate::ExternalProviderPromptOutcome {
                    output: completion.response,
                    stop_reason: completion.stop_reason,
                    permission_refusal_reason: self
                        .connection
                        .take_permission_refusal(&approval_operation),
                })
            }
            SessionOwnerReply::InputUnsent { reason, .. } => {
                if reason == RevokeReason::SkillSelectionChanged {
                    Err(ExternalProviderRuntimeError::Behavior(
                        crate::ProviderError::InvalidSelection,
                    ))
                } else if reason == RevokeReason::SettingsUnresolved {
                    Err(ExternalProviderRuntimeError::SettingsUnresolved {
                        answer: reason.human_phrase().to_owned(),
                    })
                } else {
                    Err(fixture_error(reason.human_phrase()))
                }
            }
            _ if self.connection.frame_limit_exceeded() => {
                Err(ExternalProviderRuntimeError::FrameLimitExceeded)
            }
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub fn session_activity(&self, id: String) -> FixtureResult<crate::ProviderSessionActivity> {
        use crate::ProviderSessionActivity;
        let snapshot = self.session(&id)?.owner.snapshots().borrow().clone();
        Ok(match (snapshot.lifecycle, snapshot.turn) {
            (SessionLifecycle::Live { .. }, SessionTurn::Idle) => ProviderSessionActivity::Idle,
            (SessionLifecycle::Live { .. }, _) => ProviderSessionActivity::Running,
            _ => ProviderSessionActivity::NotLoaded,
        })
    }

    pub async fn cancel_active_prompt(&self, id: String) -> FixtureResult<()> {
        self.cancel_with_report_fixture(id).await.map(|_| ())
    }

    pub async fn cancel_with_report_fixture(&self, id: String) -> FixtureResult<CancelDrainReport> {
        let session = self.session(&id)?;
        let snapshot = session.owner.snapshots().borrow().clone();
        let SessionTurn::Running { turn } = snapshot.turn else {
            return Err(ExternalProviderRuntimeError::LocalCancelTargetMismatch);
        };
        let mut reports = {
            let state = lock(&session.state);
            state.cancel_report.send_replace(None);
            state.cancel_report.subscribe()
        };
        session
            .admit(SessionOwnerCommand::CancelTurn {
                generation: session.generation()?,
                turn,
            })
            .await?;
        let result = reports
            .wait_for(Option::is_some)
            .await
            .map_err(fixture_error)?;
        result
            .clone()
            .ok_or(ExternalProviderRuntimeError::TransportFailure)?
            .map_err(|reason| fixture_error(reason.human_phrase()))
    }

    pub async fn end_session_actor(&self, id: String) -> FixtureResult<()> {
        let session = self.session(&id)?;
        let generation = session.generation()?;
        let completion = session
            .owner
            .completion_port()
            .reserve(session.address)
            .map_err(fixture_error)?;
        completion.send(provider_session_machines::session_owner::OwnerEvent::RunLost(generation));
        session
            .owner
            .snapshots()
            .wait_for(|snapshot| matches!(snapshot.lifecycle, SessionLifecycle::NotLoaded { .. }))
            .await
            .map_err(fixture_error)?;
        Ok(())
    }

    pub fn signal_run_loss_fixture(&self, id: &str) -> FixtureResult<()> {
        let session = self.session(id)?;
        let generation = session.generation()?;
        let completion = session
            .owner
            .completion_port()
            .reserve(session.address)
            .map_err(fixture_error)?;
        tokio::spawn(async move {
            crate::sim_points::reach(crate::sim_points::ClientSimPoint::RunLossReady).await;
            completion
                .send(provider_session_machines::session_owner::OwnerEvent::RunLost(generation));
        });
        Ok(())
    }

    pub async fn accept_session_settings(
        &self,
        id: String,
    ) -> FixtureResult<EffectiveProviderSettings> {
        let session = self.session(&id)?;
        session
            .admit(SessionOwnerCommand::AcceptSettings {
                generation: session.generation()?,
            })
            .await
            .map_err(|_| ExternalProviderRuntimeError::SettingsOutcomeUncertain)?;
        Ok(session.effective_settings())
    }

    pub async fn steer_contents_with_input(
        &self,
        id: String,
        input_id: InputId,
        submission: session_event_model::ProviderSubmission,
    ) -> FixtureResult<crate::ProviderSteeringOutcome<P::OperationId, P::Context>> {
        use crate::ProviderSteeringOutcome;
        let session = self.session(&id)?;
        let capabilities = lock(&session.state).capabilities.clone();
        let content =
            self.prepare_owner_prompt(ProviderTurnInput::Prompt(submission), &capabilities)?;
        let operation = Uuid::now_v7();
        let snapshot = session.owner.snapshots().borrow().clone();
        let turn = match snapshot.turn {
            SessionTurn::Running { turn } => turn,
            _ => TurnId(Uuid::now_v7()),
        };
        let joined = lock(&session.state)
            .active_context
            .as_ref()
            .filter(|(active, _)| *active == turn)
            .map(|(_, context)| context.clone());
        let input = OwnerInput {
            generation: session.generation()?,
            operation,
            input: input_id,
            turn,
        };
        lock(&session.state).inputs.insert(
            input.input.clone(),
            FixtureInput {
                content,
                context: None,
                dispatch: None,
            },
        );
        let reply = session
            .settle(
                operation,
                SessionOwnerCommand::Steer {
                    input,
                    supported: capabilities.supports_steering,
                },
            )
            .await?;
        Ok(match reply {
            SessionOwnerReply::InputFinished {
                remote: RemoteEffect::Applied,
                ..
            } => ProviderSteeringOutcome::Injected {
                running_operation_id: joined.as_ref().map(P::operation_id),
                joined_turn: joined,
            },
            SessionOwnerReply::SteerStarted { .. } | SessionOwnerReply::PromptCompleted { .. } => {
                ProviderSteeringOutcome::StartedNewTurn
            }
            SessionOwnerReply::SteerInvalidReply { .. } => {
                return Err(fixture_error(
                    "provider returned an invalid steering result",
                ));
            }
            _ => ProviderSteeringOutcome::Failed,
        })
    }

    pub fn in_folder_router_auto_for_test(&self, id: &str) -> Option<bool> {
        let wire = self.owner_wire();
        wire.router_auto_grant_for_test(wire.route_for_session(id)?)
    }

    pub fn settings_catalog(&self, id: &str) -> Option<ProviderSettingsCatalog> {
        let session = self.session(id).ok()?;
        let mut catalog =
            self.owner_wire()
                .session_catalog(crate::acp_transport::TransportRun {
                    incarnation: session.address.incarnation,
                    generation: session.generation().ok()?,
                })?;
        catalog.current_mode = session.effective_settings().mode;
        if catalog.current_mode.is_none() {
            catalog.mode_observation = crate::ProviderModeObservation::Unconfirmed;
        }
        Some(catalog)
    }

    pub async fn capability_report(&self, id: &str) -> ProviderCapabilityReport {
        let connection = self.connection.connection_capability_report().await;
        let observed = self.session(id).ok().and_then(|session| {
            session
                .owner
                .snapshots()
                .borrow()
                .projection
                .capabilities
                .clone()
        });
        match observed {
            Some(observed) => connection.with_session_model(&observed),
            None => connection,
        }
    }

    pub async fn wait_for_input(&self, id: &str, input_id: &InputId) -> FixtureResult<OwnerInput> {
        let owner = self.session(id)?;
        let mut snapshots = owner.owner.snapshots();
        let waiting = snapshots
            .wait_for(|snapshot| {
                snapshot
                    .queue
                    .iter()
                    .any(|entry| &entry.input.input == input_id)
            })
            .await
            .map_err(fixture_error)?;
        waiting
            .queue
            .iter()
            .find(|entry| &entry.input.input == input_id)
            .map(|entry| entry.input.clone())
            .ok_or(ExternalProviderRuntimeError::LocalNotFound)
    }

    pub fn settings_unresolved(&self, id: &str) -> bool {
        self.session(id)
            .is_ok_and(|session| session.owner.snapshots().borrow().settings_block_input)
    }

    pub async fn shutdown(&self) {
        self.connection.shutdown().await;
        let sessions: Vec<_> = lock(&self.sessions).values().cloned().collect();
        for session in sessions {
            let mut snapshots = session.owner.snapshots();
            let _settled = snapshots
                .wait_for(|snapshot| {
                    matches!(snapshot.lifecycle, SessionLifecycle::NotLoaded { .. })
                })
                .await;
            session.stop.cancel();
        }
    }
}

fn report_prompt_refusal(
    dispatch: Option<oneshot::Sender<ProviderPromptDispatchObservation>>,
    error: &ExternalProviderRuntimeError,
) {
    use crate::ProviderDispatchRefusal;
    if let Some(dispatch) = dispatch {
        let refusal = match error {
            ExternalProviderRuntimeError::Behavior(reason) => {
                Some(ProviderDispatchRefusal::Behavior(*reason))
            }
            ExternalProviderRuntimeError::UnsupportedContent { content_type } => {
                Some(ProviderDispatchRefusal::UnsupportedContent(content_type))
            }
            ExternalProviderRuntimeError::SettingsUnresolved { answer } => {
                Some(ProviderDispatchRefusal::SettingsUnresolved {
                    answer: answer.clone(),
                })
            }
            ExternalProviderRuntimeError::LocalBusy { answer } => {
                Some(ProviderDispatchRefusal::Busy {
                    answer: answer.clone(),
                })
            }
            _ => None,
        };
        let _caller_gone = dispatch.send(refusal.map_or(
            ProviderPromptDispatchObservation::NotSubmitted,
            ProviderPromptDispatchObservation::Refused,
        ));
    }
}

impl<P: InteractionPort> Drop for FixtureSession<P> {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl<P: InteractionPort> FixtureSession<P> {
    fn generation(&self) -> FixtureResult<SessionGeneration> {
        match self.owner.snapshots().borrow().lifecycle {
            SessionLifecycle::Live { generation }
            | SessionLifecycle::Loading { generation, .. } => Ok(generation),
            _ => Err(ExternalProviderRuntimeError::LocalNotFound),
        }
    }

    async fn admit(&self, command: SessionOwnerCommand) -> FixtureResult<()> {
        self.owner
            .try_submit(self.address, command)
            .map_err(fixture_error)?
            .await
            .map_err(fixture_error)?
            .map_err(|rejection| {
                use provider_session_machines::session_owner::{SessionOwnerRefusal, SessionOwnerRejection};
                match rejection.rejection {
                    SessionOwnerRejection::Refused(SessionOwnerRefusal::SettingsUnresolved) =>
                        ExternalProviderRuntimeError::SettingsUnresolved {
                            answer: "settings-uncertain: resolve the unknown setting before sending input".into(),
                        },
                    SessionOwnerRejection::Refused(SessionOwnerRefusal::SettingContextChanged) =>
                        ExternalProviderRuntimeError::SettingsContextChanged,
                    SessionOwnerRejection::Refused(SessionOwnerRefusal::CannotSteer) =>
                        ExternalProviderRuntimeError::UnsupportedSteering,
                    SessionOwnerRejection::Refused(SessionOwnerRefusal::SkillSelectionChanged) =>
                        ExternalProviderRuntimeError::Behavior(crate::ProviderError::InvalidSelection),
                    SessionOwnerRejection::Refused(SessionOwnerRefusal::EffectInProgress(provider_session_machines::session_owner::EffectInProgress::TurnInProgress)) =>
                        ExternalProviderRuntimeError::LocalBusy { answer: "a turn is running; queue the message or cancel the turn".into() },
                    other => ExternalProviderRuntimeError::LocalBusy { answer: format!("{other:?}") },
                }
            })
    }

    async fn settle(
        &self,
        operation: Uuid,
        command: SessionOwnerCommand,
    ) -> FixtureResult<SessionOwnerReply> {
        let (reply, answer) = oneshot::channel();
        lock(&self.state).replies.insert(operation, reply);
        if let Err(error) = self.admit(command).await {
            lock(&self.state).replies.remove(&operation);
            return Err(error);
        }
        answer.await.map_err(fixture_error)
    }

    fn effective_settings(&self) -> EffectiveProviderSettings {
        use provider_session_machines::{ProviderSettingKind, session_owner::ObservedSetting};
        let current = self.owner.snapshots();
        let snapshot = current.borrow();
        let confirmed = |kind| match &snapshot.settings.get(&kind)?.value {
            ObservedSetting::Confirmed(value) => Some(value.0.clone()),
            ObservedSetting::LastConfirmed(_) | ObservedSetting::Unknown => None,
        };
        EffectiveProviderSettings {
            mode: confirmed(ProviderSettingKind::Mode),
            model: confirmed(ProviderSettingKind::Model),
            effort: confirmed(ProviderSettingKind::Effort),
            fast: confirmed(ProviderSettingKind::Fast).map(|state| {
                if state == "off" {
                    crate::FastObserved::Off
                } else {
                    crate::FastObserved::On
                }
            }),
        }
    }
}

fn fixture_error(error: impl std::fmt::Display) -> ExternalProviderRuntimeError {
    ExternalProviderRuntimeError::Operation(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
