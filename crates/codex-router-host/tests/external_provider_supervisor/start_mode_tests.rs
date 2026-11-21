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
/// and the next prompt proceeds after the definite refusal.
#[cfg(unix)]
#[tokio::test]
async fn confirmed_fallback_then_later_failure_keeps_the_mode_report() -> TestResult {
    let fixture = ModeFixture::start(
        "claude",
        &[
            "clamp-auto",
            "offer-fast",
            "refuse-fast-once",
            "plain-prompts",
        ],
    )
    .await?;
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
    operation(prompted)?;
    let prompt_settled = operation(wait(&fixture.backend, prompt_operation).await)?;
    ensure!(matches!(
        prompt_settled.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. },
        }
    ));
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
    let wire: serde_json::Value = serde_json::from_slice(&std::fs::read(&fixture.receipt)?)?;
    ensure_eq!(
        wire.get("prompts"),
        Some(&serde_json::json!([target.session_id]))
    );
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

    /// Every mode value the agent was asked to switch to, in order.
    fn mode_values_seen(&self) -> TestResult<Vec<String>> {
        let receipt: Value = serde_json::from_slice(&std::fs::read(&self.receipt)?)?;
        Ok(receipt
            .get("modeRequests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|request| {
                request
                    .get("value")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect())
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

/// An unoffered auto leaves the named record untouched. The same session
/// accepts input, then a confirmed change survives reopening the store.
#[cfg(unix)]
#[tokio::test]
async fn a_no_wire_mode_change_keeps_the_saved_mode_and_allows_input() -> TestResult {
    let fixture = ModeFixture::start(
        "claude",
        &[
            "advertise-restore",
            "advertise-close",
            "restored-in-auto",
            "narrow-restore-modes",
            "plain-prompts",
        ],
    )
    .await?;
    let target = fixture.create_in("acceptEdits").await?;
    fixture.close(&target).await?;
    let restored = fixture.load(&target).await?;
    ensure!(matches!(
        restored,
        ConversationOperationWaitOutput::Available { .. }
    ));

    let no_wire = fixture.set_mode(&target, "auto").await;
    let accept = fixture
        .backend
        .settings_accept(collaboration_protocol::ProviderSettingsAcceptRequest {
            target: target.clone(),
            actor: fixture.creator.clone().into(),
        })
        .await;
    let before_confirmed = fixture.recorded_mode(&target).await?;
    let mode_sets_before_confirmed = fixture.mode_values_seen()?;
    let prompt_id = OperationId::generate();
    operation(
        fixture
            .backend
            .prompt(ConversationPromptRequest {
                delivery: collaboration_protocol::MessageDelivery::Auto,
                input_id: None,
                operation_id: prompt_id.clone(),
                target: target.clone(),
                generation: Some(generation()?),
                requested_by: fixture.creator.clone().into(),
                approver: fixture.creator.clone().into(),
                prompt: MessageContent::HumanUser {
                    text: MessageText::try_from("after restore".to_owned())?,
                },
            })
            .await,
    )?;
    let prompted = operation(wait(&fixture.backend, prompt_id).await)?.output;
    let agent: Value = serde_json::from_slice(&std::fs::read(&fixture.receipt)?)?;
    let repaired = fixture.set_mode(&target, "plan").await;
    let mode_sets_after_confirmed = fixture.mode_values_seen()?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    no_wire.map_err(|error| format!("auto Keep failed: {error}"))?;
    ensure!(accept.is_ok());
    ensure!(matches!(
        prompted,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    ensure_eq!(agent["prompts"], json!([target.session_id]));
    let after_confirmed = fixture.recorded_mode(&target).await?;
    ensure_eq!(before_confirmed.as_deref(), Some("acceptEdits"));
    ensure_eq!(mode_sets_before_confirmed, ["acceptEdits"]);
    repaired.map_err(|error| format!("advertised repair failed: {error}"))?;
    ensure_eq!(after_confirmed.as_deref(), Some("plan"));
    ensure_eq!(mode_sets_after_confirmed, ["acceptEdits", "plan"]);
    Ok(())
}

/// Oracle: the Lead's V6b ruling: explicit mode changes on one session are
/// recorded in the order the agent applied them. Change A (plan) is applied
/// and held before its record write. Change B (acceptEdits) is submitted and
/// waits for the session's turn without reaching the agent. A is released,
/// then B runs. The record and show end at B (wrong build: without the turn,
/// B applies and records while A is held, and A's late write leaves the
/// record at A).
#[cfg(unix)]
#[tokio::test]
async fn explicit_mode_changes_are_recorded_in_the_order_the_agent_applied_them() -> TestResult {
    let fixture = ModeFixture::start("claude", &[]).await?;
    let target = fixture.create_in("default").await?;
    let (a_held, release_a) = fixture
        .backend
        .pause_explicit_mode_record_of(&target, "plan");
    let b_waits = fixture
        .backend
        .notice_next_explicit_mode_wait(&target)
        .ok_or("the binding is known")?;

    // A is applied and held before its record write.
    let change_a = fixture.set_mode(&target, "plan");
    tokio::pin!(change_a);
    tokio::select! {
        biased;
        held = a_held => held.map_err(|_| "A's pause was dropped")?,
        ended = &mut change_a => {
            return Err(format!("A ended instead of holding before its write: {ended:?}").into());
        }
    }
    // B either waits for the session's turn or, with no turn, runs to its end.
    let change_b = fixture.set_mode(&target, "acceptEdits");
    tokio::pin!(change_b);
    let b_ended_while_a_held = tokio::select! {
        biased;
        waiting = b_waits => {
            waiting.map_err(|_| "B's wait notice was dropped")?;
            None
        }
        ended = &mut change_b => Some(ended),
    };
    let reached_agent_while_a_held = fixture.mode_values_seen()?;
    let _released = release_a.send(());
    let ended_a = change_a.await;
    let ended_b = match b_ended_while_a_held {
        Some(ended) => ended,
        None => change_b.await,
    };
    let recorded = fixture.recorded_mode(&target).await?;
    let inspected = fixture
        .backend
        .inspect_session(ProviderSessionInspectRequest {
            target: target.clone(),
        })
        .await
        .map_err(|error| format!("{error:?}"))?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    ensure!(ended_a.is_ok());
    ensure!(ended_b.is_ok());
    ensure_eq!(recorded.as_deref(), Some("acceptEdits"));
    ensure_eq!(inspected.requested_mode.as_deref(), Some("acceptEdits"));
    ensure!(!reached_agent_while_a_held.contains(&"acceptEdits".to_owned()));
    Ok(())
}

impl ModeFixture {
    async fn close(&self, target: &SessionRef) -> TestResult<()> {
        let operation_id = OperationId::generate();
        operation(
            self.backend
                .close(ConversationCloseRequest {
                    operation_id: operation_id.clone(),
                    target: target.clone(),
                    generation: Some(generation()?),
                    requested_by: self.creator.clone().into(),
                    approver: self.creator.clone().into(),
                })
                .await,
        )?;
        operation(wait(&self.backend, operation_id).await)?;
        Ok(())
    }

    /// Loads `target` and answers the settlement's output.
    async fn load(&self, target: &SessionRef) -> TestResult<ConversationOperationWaitOutput> {
        let operation_id = OperationId::generate();
        operation(
            self.backend
                .load(ConversationLoadRequest {
                    operation_id: operation_id.clone(),
                    target: target.clone(),
                    generation: Some(generation()?),
                    working_directory: working_directory()?,
                    requested_by: self.creator.clone().into(),
                    approver: self.creator.clone().into(),
                    requested_policy: policy(),
                })
                .await,
        )?;
        Ok(operation(wait(&self.backend, operation_id).await)?.output)
    }

    /// The mode the agent reports the session in now.
    async fn live_mode(&self, target: &SessionRef) -> TestResult<Option<String>> {
        Ok(self
            .backend
            .inspect_session(ProviderSessionInspectRequest {
                target: target.clone(),
            })
            .await
            .map_err(|error| format!("{error:?}"))?
            .settings_catalog
            .and_then(|catalog| catalog.current_mode))
    }
}

/// T5. A confirmed change to plan is held in the store lane. A close retires
/// the run while that accepted write is held. The later restore reads and
/// preserves the saved mode.
#[cfg(unix)]
#[tokio::test]
async fn a_restore_waits_for_an_applied_mode_change_still_being_recorded() -> TestResult {
    let fixture = ModeFixture::start("claude", &["advertise-restore", "advertise-close"]).await?;
    let target = fixture.create_in("default").await?;
    let (a_held, release_a) = fixture
        .backend
        .pause_explicit_mode_record_of(&target, "plan");

    let change_a = fixture.set_mode(&target, "plan");
    tokio::pin!(change_a);
    tokio::select! {
        biased;
        held = a_held => held.map_err(|_| "A's pause was dropped")?,
        ended = &mut change_a => {
            return Err(format!("A ended instead of holding before its write: {ended:?}").into());
        }
    }
    let close = fixture.close(&target);
    tokio::pin!(close);
    tokio::select! {
        biased;
        retired = fixture.backend.wait_for_session_retirement_fixture(&target) => retired?,
        ended = &mut close => return Err(format!("close settled before the earlier write: {ended:?}").into()),
    };
    let mode_before_release = fixture.recorded_mode(&target).await?;
    let _released = release_a.send(());
    let ended_a = change_a.await;
    close.await?;
    let reloaded = fixture.load(&target).await?;
    let recorded = fixture.recorded_mode(&target).await?;
    let live = fixture.live_mode(&target).await?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    if let Err(error) = ended_a {
        return Err(format!("the agent applied A: {error}").into());
    }
    ensure!(matches!(
        reloaded,
        ConversationOperationWaitOutput::Available { .. }
    ));
    ensure_eq!(mode_before_release.as_deref(), Some("default"));
    ensure_eq!(recorded.as_deref(), Some("plan"));
    ensure_eq!(live.as_deref(), Some("plan"));
    Ok(())
}

/// R5: dropping B at the mode-turn wait sends no B request and leaves A's
/// confirmed mode in the record. A later load of the still-live session
/// joins the same mode turn, making the absence of B's write observable.
#[cfg(all(unix, feature = "test-support"))]
#[tokio::test]
async fn a_mode_change_cancelled_while_waiting_sends_nothing() -> TestResult {
    let fixture = ModeFixture::start("claude", &["advertise-restore"]).await?;
    let target = fixture.create_in("default").await?;
    let (a_held, release_a) = fixture
        .backend
        .pause_explicit_mode_record_of(&target, "plan");
    let mut change_a = Box::pin(fixture.set_mode(&target, "plan"));
    tokio::select! {
        held = a_held => held.map_err(|_| "A's record pause was dropped")?,
        result = &mut change_a => return Err(format!("A settled before its record pause: {result:?}").into()),
    }

    let b_waiting = fixture
        .backend
        .notice_next_explicit_mode_wait(&target)
        .ok_or("the binding is known")?;
    let mut change_b = Box::pin(fixture.set_mode(&target, "acceptEdits"));
    tokio::select! {
        waiting = b_waiting => waiting.map_err(|_| "B's mode-turn wait notice was dropped")?,
        result = &mut change_b => return Err(format!("B settled before taking the turn: {result:?}").into()),
    }
    drop(change_b);
    release_a.send(()).map_err(|_| "release A's record write")?;
    change_a
        .await
        .map_err(|error| format!("A did not settle: {error}"))?;

    let barrier = fixture.load(&target).await;
    let recorded = fixture.recorded_mode(&target).await?;
    let mode_values = fixture.mode_values_seen()?;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    let refused_load = barrier.expect_err("the load reaches the still-live cell");
    ensure!(refused_load.to_string().contains("Busy"));
    ensure_eq!(mode_values, ["default", "plan"]);
    ensure_eq!(recorded.as_deref(), Some("plan"));
    Ok(())
}

/// Oracle: review F3, "the admitted change keeps ownership through its
/// settlement (it survives a caller drop)" (wrong build: the change runs in
/// its caller's future). A change to plan is applied and held before its
/// record write, and its caller stops waiting. Released, its record is still
/// written: a later change, held at the same point once applied, finds the
/// record at plan.
#[cfg(unix)]
#[tokio::test]
async fn an_applied_mode_change_is_recorded_after_its_caller_stops_waiting() -> TestResult {
    let fixture = ModeFixture::start("claude", &[]).await?;
    let target = fixture.create_in("default").await?;
    let (a_held, release_a) = fixture
        .backend
        .pause_explicit_mode_record_of(&target, "plan");

    {
        let change_a = fixture.set_mode(&target, "plan");
        tokio::pin!(change_a);
        tokio::select! {
            biased;
            held = a_held => held.map_err(|_| "A's pause was dropped")?,
            ended = &mut change_a => {
                return Err(format!("A ended instead of holding before its write: {ended:?}").into());
            }
        }
        // A's caller stops waiting here.
    }
    let (b_held, release_b) = fixture
        .backend
        .pause_explicit_mode_record_of(&target, "acceptEdits");
    let _released_a = release_a.send(());
    let change_b = fixture.set_mode(&target, "acceptEdits");
    tokio::pin!(change_b);
    tokio::select! {
        biased;
        held = b_held => held.map_err(|_| "B's pause was dropped")?,
        ended = &mut change_b => {
            return Err(format!("B ended instead of holding before its write: {ended:?}").into());
        }
    }
    let recorded_once_a_settled = fixture.recorded_mode(&target).await?;
    let _released_b = release_b.send(());
    let ended_b = change_b.await;
    fixture.backend.shutdown().await.map_err(str::to_owned)?;

    if let Err(error) = ended_b {
        return Err(format!("the agent applied B: {error}").into());
    }
    ensure_eq!(recorded_once_a_settled.as_deref(), Some("plan"));
    Ok(())
}
