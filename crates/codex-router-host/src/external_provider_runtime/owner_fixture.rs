//! Exercise the provider with the same owner and wire ports as the Host.

use super::*;
use crate::session_owner_calls::InputAcceptance;
use crate::session_owner_door::{SessionOwnerAddress, SessionOwnerDoor};
use acp_client_runtime::{RequestedProviderSettings, owner_session_open::OwnerSessionOpenSetup};
use collaboration_protocol::{
    ConversationBindingIdentity, ProviderBindingIdentity, ProviderIdentity, ProviderOperationKind,
    ProviderRequestedPolicy, RouterAccess, SessionId,
};
use collaboration_runtime::{
    ProviderOperationAdmission, ProviderOperationStore, ProviderSessionRecord,
    ProviderSessionResumeInputs,
};
use provider_session_machines::{
    session_lifecycle_machine::{CloseAttempt, SessionLifecycle},
    session_owner::{
        CloseOutcome, CloseRequest, OwnerInput, PromptAdmission, RestoreDisposition, RunAttempt,
        SessionOwnerCommand, SessionOwnerReply,
    },
    session_turn_machine::{SessionTurn, TurnId},
};
use session_event_model::{InputId, ProviderSubmission};
use tokio::sync::Mutex;
use uuid::Uuid;

pub(super) struct RuntimeFixture {
    _directory: Option<tempfile::TempDir>,
    store: Arc<Mutex<ProviderOperationStore>>,
    binding: ProviderBindingIdentity,
    door: Arc<SessionOwnerDoor>,
}

impl RuntimeFixture {
    fn target(&self, provider_id: String) -> Result<SessionRef, ExternalProviderRuntimeError> {
        let session_id = SessionId::try_from(provider_id)
            .map_err(|_| ExternalProviderRuntimeError::LocalNotFound)?;
        Ok(SessionRef {
            endpoint: self.binding.endpoint.clone(),
            session_id,
        })
    }

    fn owner(
        &self,
        target: &SessionRef,
    ) -> Result<Arc<SessionOwnerAddress>, ExternalProviderRuntimeError> {
        self.door
            .open(
                target.clone(),
                crate::provider_work_roots::ProviderWorkRoots::default(),
            )
            .map_err(fixture_error)
    }

    async fn admit(
        &self,
        operation: OperationId,
        kind: ProviderOperationKind,
        target: Option<&SessionRef>,
    ) -> Result<Uuid, ExternalProviderRuntimeError> {
        let correlation = Uuid::parse_str(operation.as_str()).map_err(fixture_error)?;
        let mut store = self.store.lock().await;
        store
            .admit(ProviderOperationAdmission {
                operation_id: operation.clone(),
                operation_kind: kind,
                binding: ConversationBindingIdentity::ExternalProvider {
                    binding: self.binding.clone(),
                },
                admitted_at_ms: 0,
            })
            .await
            .map_err(fixture_error)?;
        if let Some(target) = target {
            store
                .record_target(&operation, target, 0)
                .await
                .map_err(fixture_error)?;
        }
        if kind == ProviderOperationKind::ConversationCreate || kind.is_restore() {
            store
                .mark_may_have_dispatched(&operation, 0)
                .await
                .map_err(fixture_error)?;
        }
        Ok(correlation)
    }

    fn record(
        target: SessionRef,
        cwd: PathBuf,
    ) -> Result<ProviderSessionRecord, ExternalProviderRuntimeError> {
        let actor = ProviderIdentity::from(target.clone());
        Ok(ProviderSessionRecord {
            target,
            working_directory: cwd
                .display()
                .to_string()
                .try_into()
                .map_err(fixture_error)?,
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: actor.clone(),
            approver: actor,
            updated_at_ms: 0,
            resume: ProviderSessionResumeInputs::default(),
        })
    }
}

impl ExternalProviderRuntime {
    async fn fixture(&self) -> Result<&RuntimeFixture, ExternalProviderRuntimeError> {
        self.fixture
            .get_or_try_init(|| async {
                let binding = crate::session_store_lane::tests::mock_binding();
                let (directory, store, hub, binding) = if let Some((hub, endpoint)) =
                    &self.fixture_hub
                {
                    let endpoint = serde_json::from_value(
                        serde_json::to_value(endpoint).map_err(fixture_error)?,
                    )
                    .map_err(fixture_error)?;
                    let binding = ProviderBindingIdentity {
                        endpoint,
                        ..binding
                    };
                    (None, hub.store_for_fixture(), hub.clone(), binding)
                } else {
                    let directory = tempfile::tempdir().map_err(fixture_error)?;
                    let store = Arc::new(Mutex::new(
                        ProviderOperationStore::open(&directory.path().join("operations.sqlite"))
                            .await
                            .map_err(fixture_error)?,
                    ));
                    let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
                    (Some(directory), store, hub, binding)
                };
                let door = self.session_owner_door(store.clone(), hub);
                Ok(RuntimeFixture {
                    _directory: directory,
                    store,
                    binding,
                    door,
                })
            })
            .await
    }

    pub async fn create_session(
        &self,
        cwd: PathBuf,
    ) -> Result<String, ExternalProviderRuntimeError> {
        self.create_session_with_settings(cwd, RequestedProviderSettings::default())
            .await
    }

    pub(super) async fn create_session_with_settings(
        &self,
        cwd: PathBuf,
        settings: RequestedProviderSettings,
    ) -> Result<String, ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let operation = OperationId::generate();
        fixture
            .admit(
                operation.clone(),
                ProviderOperationKind::ConversationCreate,
                None,
            )
            .await?;
        let created = fixture
            .door
            .create(
                fixture.binding.endpoint.clone(),
                operation.clone(),
                OwnerSessionOpenSetup {
                    cwd: cwd.clone(),
                    work_roots: Default::default(),
                    scratch_folder: None,
                },
                settings,
                Default::default(),
            )
            .await?;
        let id = created.created.provider_session_id;
        let record = RuntimeFixture::record(fixture.target(id.clone())?, cwd)?;
        fixture
            .store
            .lock()
            .await
            .settle_session_operation(&operation, &record)
            .await
            .map_err(fixture_error)?;
        Ok(id)
    }

    pub async fn load_session(
        &self,
        provider_id: String,
        cwd: PathBuf,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id)?;
        let operation = fixture
            .admit(
                OperationId::generate(),
                ProviderOperationKind::ConversationLoad,
                Some(&target),
            )
            .await?;
        let result = fixture
            .owner(&target)?
            .restore_explicit(
                operation,
                RestoreVia::Load,
                Box::new(RuntimeFixture::record(target, cwd)?),
            )
            .await
            .map_err(|error| match error {
                crate::session_owner_door::OwnerDoorError::Refused(_) => {
                    ExternalProviderRuntimeError::LocalBusy {
                        answer: error.to_string(),
                    }
                }
                error => fixture_error(error),
            })?;
        match result.reply {
            SessionOwnerReply::RestoreFinished(result) => match result.disposition {
                RestoreDisposition::Restored { .. } => Ok(()),
                RestoreDisposition::Retired {
                    failure: provider_session_machines::session_owner::RestoreFailure::Closed,
                    ..
                } => Err(ExternalProviderRuntimeError::SessionReleased),
                RestoreDisposition::Retired { failure, .. }
                    if let Some(coded) = failure.coded_evidence() =>
                {
                    Err(coded.into())
                }
                RestoreDisposition::Retired {
                    failure: provider_session_machines::session_owner::RestoreFailure::Disconnected,
                    ..
                } => Err(ExternalProviderRuntimeError::TransportFailure),
                RestoreDisposition::Retired { failure, .. } => {
                    Err(fixture_error(failure.next_step()))
                }
            },
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn close_session(
        &self,
        provider_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.fixture_close(provider_id, true).await
    }

    pub async fn release_session(
        &self,
        provider_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.fixture_close(provider_id, false).await
    }

    async fn fixture_close(
        &self,
        provider_id: String,
        supported: bool,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id)?;
        let owner = fixture.owner(&target)?;
        let generation = match owner.snapshots().borrow().lifecycle {
            SessionLifecycle::Live { generation }
            | SessionLifecycle::Loading { generation, .. } => generation,
            _ => return Err(ExternalProviderRuntimeError::LocalNotFound),
        };
        let operation = fixture
            .admit(
                OperationId::generate(),
                ProviderOperationKind::ConversationClose,
                Some(&target),
            )
            .await?;
        let result = owner
            .settle(
                operation,
                SessionOwnerCommand::CloseWithCapability(CloseRequest {
                    run: RunAttempt {
                        generation,
                        attempt: CloseAttempt(operation),
                    },
                    provider_supports_close: supported,
                }),
            )
            .await
            .map_err(fixture_error)?;
        match result.reply {
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

    pub async fn session_activity(
        &self,
        provider_id: String,
    ) -> Result<acp_client_runtime::ProviderSessionActivity, ExternalProviderRuntimeError> {
        use acp_client_runtime::ProviderSessionActivity;
        let fixture = self.fixture().await?;
        let Some(owner) = fixture.door.existing(&fixture.target(provider_id)?) else {
            return Ok(ProviderSessionActivity::NotLoaded);
        };
        let snapshot = owner.snapshots().borrow().clone();
        Ok(match (snapshot.lifecycle, snapshot.turn) {
            (SessionLifecycle::Live { .. }, SessionTurn::Idle) => ProviderSessionActivity::Idle,
            (SessionLifecycle::Live { .. }, _) => ProviderSessionActivity::Running,
            _ => ProviderSessionActivity::NotLoaded,
        })
    }

    pub async fn prompt(
        &self,
        provider_id: String,
        prompt: String,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.prompt_with_approval_context(
            provider_id.clone(),
            prompt,
            test_approval_context(&provider_id, None),
        )
        .await
    }

    pub async fn prompt_with_approval_context(
        &self,
        provider_id: String,
        prompt: String,
        context: ExternalProviderApprovalContext,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.prompt_from_fixture(provider_id, prompt, context, None)
            .await
    }

    pub(super) async fn prompt_for_operation(
        &self,
        provider_id: String,
        operation: Option<OperationId>,
        prompt: String,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.prompt_from_fixture(
            provider_id.clone(),
            prompt,
            test_approval_context(&provider_id, operation),
            dispatch,
        )
        .await
    }

    async fn prompt_from_fixture(
        &self,
        provider_id: String,
        prompt: String,
        context: ExternalProviderApprovalContext,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id.clone())?;
        let owner = fixture.owner(&target)?;
        let SessionLifecycle::Live { generation } = owner.snapshots().borrow().lifecycle else {
            return Err(ExternalProviderRuntimeError::LocalNotFound);
        };
        let operation = fixture
            .admit(
                context.operation_id.clone(),
                ProviderOperationKind::ConversationPrompt,
                Some(&target),
            )
            .await?;
        let content = self
            .prepare_owner_prompt(
                &provider_id,
                acp_client_runtime::ProviderTurnInput::Prompt(ProviderSubmission::plain(
                    text_contents(prompt)?,
                )),
            )
            .await?;
        let input = OwnerInput {
            generation,
            operation,
            input: InputId::generate(),
            turn: TurnId(Uuid::now_v7()),
        };
        let operation_id = context.operation_id.clone();
        owner.remember_approval(input.input.clone(), context);
        let receipt = owner
            .start_delivery(
                input.clone(),
                content,
                SessionOwnerCommand::Prompt {
                    input,
                    admission: PromptAdmission::Direct,
                },
            )
            .await
            .map_err(|error| {
                use provider_session_machines::session_owner::{
                    SessionOwnerRefusal, SessionOwnerRejection,
                };
                if matches!(
                    error,
                    crate::session_owner_door::OwnerDoorError::Refused(
                        SessionOwnerRejection::Refused(SessionOwnerRefusal::EffectInProgress)
                    )
                ) {
                    ExternalProviderRuntimeError::LocalBusy {
                        answer: error.to_string(),
                    }
                } else {
                    fixture_error(error)
                }
            })?;
        if let Some(dispatch) = dispatch {
            let started = receipt.accepted.await == Ok(InputAcceptance::Started);
            let _observer_gone = dispatch.send(if started {
                ProviderPromptDispatchObservation::Submitted
            } else {
                ProviderPromptDispatchObservation::NotSubmitted
            });
        }
        let settled = receipt.terminal.await.map_err(fixture_error)?;
        match settled.reply {
            SessionOwnerReply::PromptFailed { failure, .. } => Err(failure.into()),
            SessionOwnerReply::PromptCompleted { ref completion, .. }
                if completion.local_cause
                    == Some(session_event_model::LocalCause::OutputOverflow) =>
            {
                Err(ExternalProviderRuntimeError::PromptOutputLimitExceeded)
            }
            SessionOwnerReply::PromptCompleted { completion, .. } => {
                host_prompt_outcome(acp_client_runtime::ExternalProviderPromptOutcome {
                    output: completion.response,
                    stop_reason: completion.stop_reason,
                    permission_refusal_reason: self.take_permission_refusal(&operation_id),
                })
            }
            _ if self.frame_limit_exceeded() => {
                Err(ExternalProviderRuntimeError::FrameLimitExceeded)
            }
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }
}

fn fixture_error(error: impl std::fmt::Display) -> ExternalProviderRuntimeError {
    ExternalProviderRuntimeError::Operation(error.to_string())
}

impl ExternalProviderRuntime {
    pub async fn set_setting(
        &self,
        provider_id: String,
        request: acp_client_runtime::ProviderSettingRequest,
    ) -> Result<acp_client_runtime::EffectiveProviderSettings, ExternalProviderRuntimeError> {
        use provider_session_machines::{
            session_owner::{OwnerSetting, RemoteEffect},
            session_setting_machine::{SettingApplication, SettingValueId},
        };
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id.clone())?;
        let owner = fixture.owner(&target)?;
        let SessionLifecycle::Live { generation } = owner.snapshots().borrow().lifecycle else {
            return Err(ExternalProviderRuntimeError::LocalNotFound);
        };
        let operation = Uuid::now_v7();
        let setting = OwnerSetting {
            change: RunAttempt {
                generation,
                attempt: SettingApplication(operation),
            },
            kind: request.kind,
        };
        let input = InputId::generate();
        let value = SettingValueId(request.value.clone());
        let command = match request.intent {
            session_event_model::SettingApplicationIntent::ExplicitChange => {
                owner.expect_confirmed_mode(setting, request.value.clone());
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
                    reuse_confirmed: self.can_reuse_setting_confirmation(request.kind),
                }
            }
        };
        let settled = owner
            .settle(operation, command)
            .await
            .map_err(fixture_error)?;
        match settled.reply {
            SessionOwnerReply::SettingFinished {
                remote: RemoteEffect::Unknown,
                ..
            } => Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                provider_session_id: provider_id,
                setting: request.kind,
                value: request.value,
            }),
            SessionOwnerReply::SettingFinished { .. } | SessionOwnerReply::SettingKept { .. } => {
                Ok(crate::external_provider_supervisor::snapshot_settings(
                    &owner.snapshots().borrow(),
                ))
            }
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn steer_session(
        &self,
        provider_id: String,
        prompt: String,
    ) -> Result<crate::ProviderSteeringOutcome, ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id.clone())?;
        let owner = fixture.owner(&target)?;
        let snapshot = owner.snapshots().borrow().clone();
        let SessionLifecycle::Live { generation } = snapshot.lifecycle else {
            return Err(ExternalProviderRuntimeError::LocalNotFound);
        };
        let (op, scope) = (OperationId::generate(), snapshot.turn);
        let operation = fixture
            .admit(op, ProviderOperationKind::ConversationPrompt, Some(&target))
            .await?;
        let content = self
            .prepare_owner_prompt(
                &provider_id,
                acp_client_runtime::ProviderTurnInput::Prompt(ProviderSubmission::plain(
                    text_contents(prompt)?,
                )),
            )
            .await?;
        let turn = match scope {
            SessionTurn::Running { turn } => turn,
            _ => TurnId(Uuid::now_v7()),
        };
        let input = OwnerInput {
            generation,
            operation,
            input: InputId::generate(),
            turn,
        };
        let supported = snapshot
            .projection
            .capabilities
            .is_some_and(|report| report.steer);
        let receipt = owner
            .start_delivery(
                input.clone(),
                content,
                SessionOwnerCommand::Steer { input, supported },
            )
            .await
            .map_err(fixture_error)?;
        match receipt.accepted.await.map_err(fixture_error)? {
            InputAcceptance::ProviderStarted | InputAcceptance::Started => {
                Ok(crate::ProviderSteeringOutcome::StartedNewTurn)
            }
            InputAcceptance::Steered { joined_operation } => {
                Ok(crate::ProviderSteeringOutcome::Injected {
                    running_operation_id: joined_operation
                        .and_then(|id| OperationId::try_from(id.to_string()).ok()),
                    joined_turn: None,
                })
            }
            InputAcceptance::Unsent { .. } => Ok(crate::ProviderSteeringOutcome::Failed),
            _ => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub async fn cancel_active_prompt(
        &self,
        provider_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.cancel_fixture_prompt(provider_id, None).await
    }

    pub(super) async fn cancel_prompt_operation(
        &self,
        provider_id: String,
        operation: OperationId,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let operation = Uuid::parse_str(operation.as_str()).map_err(fixture_error)?;
        self.cancel_fixture_prompt(provider_id, Some(operation))
            .await
    }

    async fn cancel_fixture_prompt(
        &self,
        provider_id: String,
        operation: Option<Uuid>,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let target = fixture.target(provider_id)?;
        let owner = fixture.owner(&target)?;
        let foreground = owner
            .snapshots()
            .borrow()
            .foreground
            .as_ref()
            .filter(|active| operation.is_none_or(|id| id == active.operation))
            .cloned()
            .ok_or_else(|| {
                ExternalProviderRuntimeError::Operation("prompt is no longer active".into())
            })?;
        owner
            .admit(SessionOwnerCommand::CancelTurn {
                generation: foreground.generation,
                turn: foreground.turn,
            })
            .await
            .map_err(fixture_error)
    }

    pub(super) async fn wait_session_idle(
        &self,
        provider_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let fixture = self.fixture().await?;
        let owner = fixture.owner(&fixture.target(provider_id)?)?;
        owner
            .snapshots()
            .wait_for(|snapshot| snapshot.turn == SessionTurn::Idle)
            .await
            .map_err(fixture_error)?;
        Ok(())
    }
}
