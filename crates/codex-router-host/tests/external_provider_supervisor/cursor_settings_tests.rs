//! Existing provider API preserves authority and exposes returned companion values.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn cursor_effort_setting_reports_adjusted_catalog_without_public_api_expansion() -> TestResult
{
    let root = tempfile::tempdir()?;
    let receipt = root.path().join("wire.json");
    let provider_endpoint = endpoint("cursor-local")?;
    let creator = actor(provider_endpoint.clone(), "creator")?;
    let mut provider_launch = ExternalProviderLaunch {
        executable: PathBuf::from("node"),
        arguments: vec![
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../verification/fixtures/cursor-settings-agent.ts"
            )
            .into(),
        ],
        environment: Vec::new(),
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    };
    provider_launch
        .arguments
        .extend(["adjusted".into(), receipt.to_string_lossy().into_owned()]);
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http(
        provider_launch,
        acp_client_runtime::ProviderModelPicker::CursorParameterized,
        "fixture",
        "http://127.0.0.1:1/mcp",
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await?;
    let backend = supervisor(
        &root,
        vec![(binding(provider_endpoint.clone(), "cursor")?, runtime)],
    )
    .await?;
    let operation_id = OperationId::generate();
    operation(
        backend
            .create(ConversationCreateRequest {
                settings: Some(collaboration_protocol::ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    // This test is about effort; --fast auto leaves the fixture's fast alone.
                    fast: Some(collaboration_protocol::FastSetting::Auto),
                    ..collaboration_protocol::ProviderRequestedSettings::default()
                }),
                operation_id: operation_id.clone(),
                endpoint: provider_endpoint.clone(),
                generation: Some(generation()?),
                working_directory: working_directory()?,
                created_by: creator.clone().into(),
                approver: creator.clone().into(),
                requested_policy: policy(),
            })
            .await,
    )?;
    let settled = operation(wait(&backend, operation_id).await)?;
    let ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::Created { target, .. },
    } = settled.output
    else {
        return Err("expected created Session".into());
    };
    let rejected = backend
        .settings_set(ProviderSettingsSetRequest {
            target: target.clone(),
            actor: actor(provider_endpoint, "stranger")?.into(),
            setting: ProviderSettingName::Effort,
            value: "high".into(),
        })
        .await
        .expect_err("only creator or Approver may change effort");
    ensure_eq!(rejected.kind, ProviderSettingsFailureKind::WrongActor);
    let applied = backend
        .settings_set(ProviderSettingsSetRequest {
            target: target.clone(),
            actor: creator.into(),
            setting: ProviderSettingName::Effort,
            value: "high".into(),
        })
        .await
        .map_err(|error| error.message)?;
    ensure_eq!(
        applied.effective_settings.model.as_deref(),
        Some("fixture-a")
    );
    ensure_eq!(applied.effective_settings.effort.as_deref(), Some("high"));
    let inspected = backend
        .inspect_session(ProviderSessionInspectRequest { target })
        .await
        .map_err(|error| error.message)?;
    let catalog = inspected
        .settings_catalog
        .ok_or("returned catalog missing")?;
    for (config_id, value) in [
        ("model", "fixture-a"),
        ("effort", "high"),
        ("thinking", "true"),
        ("context", "1m"),
        ("fast", "true"),
    ] {
        let option = catalog
            .config_options
            .iter()
            .find(|option| option.id == config_id)
            .ok_or("reported option absent")?;
        ensure_eq!(
            &option.current_value,
            &collaboration_protocol::ProviderConfigValueView::Select {
                value: value.into(),
            }
        );
        if config_id == "thinking" {
            ensure!(option.category.is_none());
        }
    }
    backend.shutdown().await.map_err(str::to_owned)?;
    let requests: Vec<Value> = serde_json::from_slice(&std::fs::read(receipt)?)?;
    ensure_eq!(requests.len(), 3);
    ensure_eq!(
        &requests[0]["params"]["clientCapabilities"]["_meta"]["parameterizedModelPicker"],
        &json!(true)
    );
    ensure_eq!(&requests[2]["method"], &json!("session/set_config_option"));
    ensure_eq!(
        &requests[2]["params"],
        &json!({
            "sessionId":"fixture-session", "configId":"effort", "value":"high"
        })
    );
    Ok(())
}

/// A Host over the reactive Cursor fixture, which starts fast on and turns it
/// back on at every model change.
struct ReactiveCursor {
    root: tempfile::TempDir,
    receipt: PathBuf,
    endpoint: EndpointRef,
    creator: SessionRef,
    backend: ExternalProviderSupervisor,
}

impl ReactiveCursor {
    async fn start(behaviours: &[&str]) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        Self::start_in(root, behaviours).await
    }

    /// A Host over an existing root, as after a restart.
    async fn start_in(root: tempfile::TempDir, behaviours: &[&str]) -> TestResult<Self> {
        // Each Host start gets its own receipt, so a restart doesn't overwrite it.
        static STARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let start = STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let receipt = root.path().join(format!("wire-{start}.json"));
        let arguments = ["reactive".to_owned(), receipt.display().to_string()]
            .into_iter()
            .chain(behaviours.iter().map(|behaviour| (*behaviour).to_owned()))
            .collect();
        let runtime = ExternalProviderRuntime::initialize(
            launch("cursor-settings-agent.ts", arguments),
            std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await?;
        let endpoint = endpoint("cursor-local")?;
        let backend =
            supervisor(&root, vec![(binding(endpoint.clone(), "cursor")?, runtime)]).await?;
        Ok(Self {
            creator: actor(endpoint.clone(), "creator")?,
            root,
            receipt,
            endpoint,
            backend,
        })
    }

    async fn create(
        &self,
        fast: Option<collaboration_protocol::FastSetting>,
    ) -> TestResult<(
        SessionRef,
        collaboration_protocol::EffectiveProviderSettings,
    )> {
        let operation_id = OperationId::generate();
        operation(
            self.backend
                .create(ConversationCreateRequest {
                    settings: Some(ProviderRequestedSettings {
                        mode: Some("auto".into()),
                        fast,
                        ..ProviderRequestedSettings::default()
                    }),
                    operation_id: operation_id.clone(),
                    endpoint: self.endpoint.clone(),
                    generation: Some(generation()?),
                    working_directory: working_directory()?,
                    created_by: self.creator.clone().into(),
                    approver: self.creator.clone().into(),
                    requested_policy: policy(),
                })
                .await,
        )?;
        match operation(wait(&self.backend, operation_id).await)?.output {
            ConversationOperationWaitOutput::Available {
                settlement:
                    ConversationOperationSettlement::Created {
                        target,
                        effective_settings,
                    },
            } => Ok((target, effective_settings)),
            other => Err(format!("expected a created session: {other:?}").into()),
        }
    }

    async fn set(
        &self,
        target: &SessionRef,
        setting: ProviderSettingName,
        value: &str,
    ) -> Result<
        collaboration_protocol::ProviderSettingsResult,
        collaboration_protocol::ProviderSettingsFailure,
    > {
        self.backend
            .settings_set(ProviderSettingsSetRequest {
                target: target.clone(),
                actor: self.creator.clone().into(),
                setting,
                value: value.into(),
            })
            .await
    }

    async fn show_fast(
        &self,
        target: &SessionRef,
    ) -> TestResult<Option<collaboration_protocol::FastState>> {
        Ok(self
            .backend
            .inspect_session(ProviderSessionInspectRequest {
                target: target.clone(),
            })
            .await
            .map_err(|error| error.message)?
            .fast)
    }

    fn setting_requests(&self) -> TestResult<usize> {
        let requests: Vec<Value> = serde_json::from_slice(&std::fs::read(&self.receipt)?)?;
        Ok(requests
            .iter()
            .filter(|request| request["method"] == "session/set_config_option")
            .count())
    }
}

/// Oracle: D1 V12, spec R7 — any fast value but off or auto is refused at the
/// Host with "fast accepts off or auto", and nothing reaches the agent.
#[cfg(unix)]
#[tokio::test]
async fn fast_values_other_than_off_or_auto_are_refused() -> TestResult {
    let cursor = ReactiveCursor::start(&[]).await?;
    let (target, _) = cursor.create(None).await?;
    let before = cursor.setting_requests()?;
    for value in ["on", "true", "fast", ""] {
        let refused = cursor
            .set(&target, ProviderSettingName::Fast, value)
            .await
            .expect_err("only off or auto");
        ensure_eq!(refused.kind, ProviderSettingsFailureKind::InvalidSetting);
        ensure_eq!(refused.message, "fast accepts off or auto".to_owned());
        ensure_eq!(refused.setting, Some(ProviderSettingName::Fast));
    }
    let after = cursor.setting_requests()?;
    cursor.backend.shutdown().await.map_err(str::to_owned)?;
    ensure_eq!(after, before);
    Ok(())
}

/// Oracle: D1 V18, spec R11 — fast is carried on create, on the settings
/// result and on show: off after the default, the agent's own on with auto.
#[cfg(unix)]
#[tokio::test]
async fn fast_is_reported_on_create_settings_and_show() -> TestResult {
    use collaboration_protocol::{FastSetting, FastState};
    let cursor = ReactiveCursor::start(&[]).await?;
    let (default_target, created) = cursor.create(None).await?;
    let set = cursor
        .set(&default_target, ProviderSettingName::Fast, "off")
        .await
        .map_err(|error| error.message)?;
    let shown = cursor.show_fast(&default_target).await?;
    let (auto_target, created_auto) = cursor.create(Some(FastSetting::Auto)).await?;
    let shown_auto = cursor.show_fast(&auto_target).await?;
    cursor.backend.shutdown().await.map_err(str::to_owned)?;
    ensure_eq!(created.fast, Some(FastState::Off));
    ensure_eq!(set.effective_settings.fast, Some(FastState::Off));
    ensure_eq!(shown, Some(FastState::Off));
    ensure_eq!(created_auto.fast, Some(FastState::On));
    ensure_eq!(shown_auto, Some(FastState::On));
    Ok(())
}

/// Oracle: D1 V19, spec R11 "absent" — after a restart, show of a stored,
/// unloaded Cursor session has no fast; once loaded, it reports the agent's.
#[cfg(unix)]
#[tokio::test]
async fn cold_host_show_does_not_claim_fast_before_load() -> TestResult {
    let first = ReactiveCursor::start(&[]).await?;
    let (target, _) = first.create(None).await?;
    first.backend.shutdown().await.map_err(str::to_owned)?;
    let ReactiveCursor { root, .. } = first;
    let restarted = ReactiveCursor::start_in(root, &[]).await?;
    let cold = restarted.show_fast(&target).await?;
    let operation_id = OperationId::generate();
    operation(
        restarted
            .backend
            .load(ConversationLoadRequest {
                operation_id: operation_id.clone(),
                target: target.clone(),
                generation: Some(generation()?),
                working_directory: working_directory()?,
                requested_by: restarted.creator.clone().into(),
                approver: restarted.creator.clone().into(),
                requested_policy: policy(),
            })
            .await,
    )?;
    operation(wait(&restarted.backend, operation_id).await)?;
    let loaded = restarted.show_fast(&target).await?;
    restarted.backend.shutdown().await.map_err(str::to_owned)?;
    ensure_eq!(cold, None);
    ensure_eq!(loaded, Some(collaboration_protocol::FastState::On));
    Ok(())
}

/// Oracle: D1 V15 Host label, spec R9 — a fast failure after a confirmed model
/// change names the setting fast and the value off, not the model.
#[cfg(unix)]
#[tokio::test]
async fn fast_failure_after_model_change_is_labelled_fast_off() -> TestResult {
    let cursor = ReactiveCursor::start(&["refuse-fast-after-model"]).await?;
    let (target, _) = cursor.create(None).await?;
    let refused = cursor
        .set(&target, ProviderSettingName::Model, "fixture-b")
        .await
        .expect_err("the fast off after the model is refused");
    cursor.backend.shutdown().await.map_err(str::to_owned)?;
    ensure_eq!(refused.setting, Some(ProviderSettingName::Fast));
    ensure_eq!(refused.value.as_deref(), Some("off"));
    Ok(())
}
