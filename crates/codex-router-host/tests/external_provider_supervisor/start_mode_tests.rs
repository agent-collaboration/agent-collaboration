//! Which mode a Claude or Cursor start must name, and what create and show
//! report about it, through the real Host create and inspect paths (D1 spec
//! R1, R2).
use super::*;

const PERMISSION_MODE_AGENT: &str = "permission-mode-agent.ts";

struct ModeFixture {
    _root: tempfile::TempDir,
    receipt: PathBuf,
    endpoint: EndpointRef,
    creator: SessionRef,
    backend: ExternalProviderSupervisor,
}

impl ModeFixture {
    async fn start(profile: &str, behaviours: &[&str]) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let receipt = root.path().join("receipt.json");
        let mut arguments = vec![
            receipt.display().to_string(),
            profile.to_owned(),
            "grants-directories".to_owned(),
        ];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        let behavior: Arc<dyn acp_client_runtime::ProviderBehavior> = if profile == "cursor" {
            Arc::new(acp_provider_adapters::CursorAcpBehavior)
        } else {
            Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
                acp_provider_adapters::ClaudeSkillSource::Annotated,
            ))
        };
        let runtime =
            ExternalProviderRuntime::initialize(launch(PERMISSION_MODE_AGENT, arguments), behavior)
                .await?;
        let name = if profile == "cursor" {
            "cursor"
        } else {
            "claude"
        };
        let endpoint = endpoint(&format!("{name}-local"))?;
        let backend = supervisor(&root, vec![(binding(endpoint.clone(), name)?, runtime)]).await?;
        Ok(Self {
            creator: actor(endpoint.clone(), "creator")?,
            _root: root,
            receipt,
            endpoint,
            backend,
        })
    }

    fn create_request(
        &self,
        operation_id: OperationId,
        settings: Option<ProviderRequestedSettings>,
    ) -> TestResult<ConversationCreateRequest> {
        Ok(ConversationCreateRequest {
            work_roots: Vec::new(),
            settings,
            operation_id,
            endpoint: self.endpoint.clone(),
            generation: Some(generation()?),
            working_directory: working_directory()?,
            created_by: self.creator.clone().into(),
            approver: self.creator.clone().into(),
            requested_policy: policy(),
        })
    }

    /// Every ACP request the fixture recorded, or none when it never saw a session.
    fn sessions_seen(&self) -> TestResult<Value> {
        if !self.receipt.exists() {
            return Ok(json!([]));
        }
        let receipt: Value = serde_json::from_slice(&std::fs::read(&self.receipt)?)?;
        Ok(receipt.get("sessions").cloned().unwrap_or(Value::Null))
    }
}

/// Oracle: D1 spec R1 — a Claude or Cursor start without a mode is refused
/// before anything starts: invalid setting, stage validation, a message that
/// names `--mode auto`; nothing is recorded and nothing reaches the agent.
#[cfg(unix)]
#[tokio::test]
async fn a_provider_create_without_a_mode_is_refused() -> TestResult {
    for profile in ["claude", "cursor"] {
        let fixture = ModeFixture::start(profile, &[]).await?;
        for settings in [None, Some(ProviderRequestedSettings::default())] {
            let operation_id = OperationId::generate();
            let refused = fixture
                .backend
                .create(fixture.create_request(operation_id.clone(), settings.clone())?)
                .await;
            let Err(refusal) = refused else {
                return Err(
                    format!("{profile} {settings:?}: a create without a mode started").into(),
                );
            };
            ensure_eq!(
                refusal.kind,
                ConversationOperationFailureKind::InvalidSetting
            );
            ensure_eq!(
                refusal.stage,
                collaboration_protocol::ConversationOperationFailureStage::Validation
            );
            ensure!(String::from(refusal.message.clone()).contains("--mode auto"));
            ensure!(
                fixture
                    .backend
                    .lookup_existing(operation_id)
                    .await
                    .map_err(|error| format!("{error:?}"))?
                    .is_none()
            );
        }
        fixture.backend.shutdown().await.map_err(str::to_owned)?;
        ensure_eq!(fixture.sessions_seen()?, json!([]));
    }
    Ok(())
}

/// Oracle: D1 spec R2 row "Its answer is lost or unreadable" and V6 steps 1
/// and 2 — the start is created without settings with the mode's outcome
/// unknown, and show reports the requested auto but no current mode.
#[cfg(unix)]
#[tokio::test]
async fn lost_auto_is_unconfirmed_on_create_and_show() -> TestResult {
    let fixture = ModeFixture::start("claude", &["malformed-mode-reply"]).await?;
    let operation_id = OperationId::generate();
    operation(
        fixture
            .backend
            .create(fixture.create_request(
                operation_id.clone(),
                Some(ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..ProviderRequestedSettings::default()
                }),
            )?)
            .await,
    )?;
    let settled = operation(wait(&fixture.backend, operation_id).await)?;
    let ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::CreatedWithoutSettings { target, failed, .. },
    } = settled.output
    else {
        return Err(format!("expected a start created without settings: {settled:?}").into());
    };
    let inspected = fixture
        .backend
        .inspect_session(ProviderSessionInspectRequest { target })
        .await
        .map_err(|error| format!("{error:?}"))?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    ensure_eq!(failed.len(), 1);
    ensure_eq!(failed[0].setting, ProviderSettingName::Mode);
    ensure_eq!(
        failed[0].reason,
        session_event_model::ProviderSettingFailureReason::OutcomeUnknown
    );
    ensure_eq!(inspected.requested_mode.as_deref(), Some("auto"));
    ensure_eq!(
        inspected
            .settings_catalog
            .ok_or("show has the settings catalog")?
            .current_mode,
        None::<String>
    );
    Ok(())
}

/// Oracle: D1 V5b, spec R2 "When the mode is confirmed but a later setting
/// fails" — the start is created without settings naming fast, with the mode
/// applied; show keeps requested auto, the confirmed fallback and its reason,
/// and prompts are held.
#[cfg(unix)]
#[tokio::test]
async fn confirmed_fallback_then_later_failure_keeps_the_mode_report() -> TestResult {
    let fixture =
        ModeFixture::start("claude", &["clamp-auto", "offer-fast", "refuse-fast-once"]).await?;
    let operation_id = OperationId::generate();
    operation(
        fixture
            .backend
            .create(fixture.create_request(
                operation_id.clone(),
                Some(ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..ProviderRequestedSettings::default()
                }),
            )?)
            .await,
    )?;
    let settled = operation(wait(&fixture.backend, operation_id).await)?;
    let ConversationOperationWaitOutput::Available {
        settlement:
            ConversationOperationSettlement::CreatedWithoutSettings {
                target,
                applied,
                failed,
                ..
            },
    } = settled.output
    else {
        return Err(format!("expected a start created without settings: {settled:?}").into());
    };
    let inspected = fixture
        .backend
        .inspect_session(ProviderSessionInspectRequest {
            target: target.clone(),
        })
        .await
        .map_err(|error| format!("{error:?}"))?;
    let prompt_operation = OperationId::generate();
    let prompted = fixture
        .backend
        .prompt(ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: prompt_operation.clone(),
            target: target.clone(),
            generation: Some(generation()?),
            requested_by: fixture.creator.clone().into(),
            approver: fixture.creator.clone().into(),
            prompt: MessageContent::HumanUser {
                text: MessageText::try_from("hello".to_owned())?,
            },
        })
        .await;
    let held = match prompted {
        Err(refusal) => refusal.kind,
        Ok(_) => match wait(&fixture.backend, prompt_operation).await {
            Err(refusal) => refusal.kind,
            Ok(result) => return Err(format!("a held prompt ran: {result:?}").into()),
        },
    };
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    ensure_eq!(failed.len(), 1);
    ensure_eq!(failed[0].setting, ProviderSettingName::Fast);
    ensure!(
        applied
            .iter()
            .any(|setting| setting.setting == ProviderSettingName::Mode)
    );
    ensure_eq!(inspected.requested_mode.as_deref(), Some("auto"));
    ensure_eq!(
        inspected.mode_reason,
        Some(collaboration_protocol::ProviderModeReason::AgentFellBack)
    );
    let current_mode = inspected
        .settings_catalog
        .ok_or("show has the settings catalog")?
        .current_mode;
    ensure_eq!(current_mode.as_deref(), Some("acceptEdits"));
    ensure_eq!(held, ConversationOperationFailureKind::SettingsUnresolved);
    Ok(())
}

impl ModeFixture {
    /// Creates a session asking for `mode` and answers its target, whether
    /// the agent applied the mode or not.
    async fn create_in(&self, mode: &str) -> TestResult<SessionRef> {
        let operation_id = OperationId::generate();
        operation(
            self.backend
                .create(self.create_request(
                    operation_id.clone(),
                    Some(ProviderRequestedSettings {
                        mode: Some(mode.into()),
                        ..ProviderRequestedSettings::default()
                    }),
                )?)
                .await,
        )?;
        match operation(wait(&self.backend, operation_id).await)?.output {
            ConversationOperationWaitOutput::Available {
                settlement:
                    ConversationOperationSettlement::Created { target, .. }
                    | ConversationOperationSettlement::CreatedWithoutSettings { target, .. },
            } => Ok(target),
            other => Err(format!("expected a created session: {other:?}").into()),
        }
    }

    async fn set_mode(&self, target: &SessionRef, mode: &str) -> Result<(), String> {
        self.backend
            .settings_set(ProviderSettingsSetRequest {
                target: target.clone(),
                actor: self.creator.clone().into(),
                setting: ProviderSettingName::Mode,
                value: mode.into(),
            })
            .await
            .map(|_| ())
            .map_err(|failure| format!("{failure:?}"))
    }

    /// The mode the session's durable record names, read through a second
    /// connection to the supervisor's store.
    async fn recorded_mode(&self, target: &SessionRef) -> TestResult<Option<String>> {
        let mut store =
            ProviderOperationStore::open(&self._root.path().join("provider-operations.sqlite"))
                .await?;
        Ok(store
            .session_record(target)
            .await?
            .ok_or("the session has a record")?
            .resume
            .requested_mode)
    }
}

/// Oracle: the Lead's V6 ruling, "an explicit mode change during the session
/// updates the recorded mode, so a restore never reverts a deliberate
/// change": a session created in default whose mode is then changed to auto
/// records auto, the mode a restore puts back, and show reports it.
#[cfg(unix)]
#[tokio::test]
async fn an_applied_explicit_mode_change_becomes_the_recorded_mode() -> TestResult {
    let fixture = ModeFixture::start("claude", &[]).await?;
    let target = fixture.create_in("default").await?;
    let recorded_at_create = fixture.recorded_mode(&target).await?;

    let changed = fixture.set_mode(&target, "auto").await;
    let recorded_after_change = fixture.recorded_mode(&target).await?;
    let inspected = fixture
        .backend
        .inspect_session(ProviderSessionInspectRequest {
            target: target.clone(),
        })
        .await
        .map_err(|error| format!("{error:?}"))?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    if let Err(error) = changed {
        return Err(format!("the agent applied the change: {error:?}").into());
    }
    ensure_eq!(recorded_at_create.as_deref(), Some("default"));
    ensure_eq!(recorded_after_change.as_deref(), Some("auto"));
    ensure_eq!(inspected.requested_mode.as_deref(), Some("auto"));
    Ok(())
}

/// Oracle: the Lead's V6 ruling, "refused or unknown leaves it": an explicit
/// mode change the agent refuses leaves the session's recorded mode as it
/// was.
#[cfg(unix)]
#[tokio::test]
async fn a_refused_explicit_mode_change_leaves_the_recorded_mode() -> TestResult {
    let fixture = ModeFixture::start("claude", &["reject-mode"]).await?;
    let target = fixture.create_in("default").await?;
    let recorded_before = fixture.recorded_mode(&target).await?;

    let changed = fixture.set_mode(&target, "plan").await;
    let recorded_after = fixture.recorded_mode(&target).await?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    if changed.is_ok() {
        return Err("the agent refused the change, but set_mode succeeded".into());
    }
    if recorded_before.is_none() {
        return Err("the create recorded a mode".into());
    }
    ensure_eq!(recorded_after, recorded_before);
    Ok(())
}
