//! Required connection authentication through the production runtime and Cursor adapter.

use super::*;

fn authentication_fixture(receipt: &std::path::Path) -> ExternalProviderLaunch {
    launch(
        "supervisor-integration-agent.ts",
        vec![
            "initialization-authentication".to_owned(),
            receipt.to_string_lossy().into_owned(),
            "success".to_owned(),
        ],
    )
}

/// Oracle: Cursor's documented initialize -> authenticate -> session flow;
/// connection model discovery also requires the authenticated connection.
#[tokio::test]
async fn initialization_authentication_precedes_models_and_is_once_per_connection() -> TestResult {
    // Arrange: an existing scripted peer requires the real Cursor handshake.
    let root = tempfile::tempdir()?;
    let receipt = root.path().join("requests.log");
    let initialized = ExternalProviderRuntime::initialize(
        authentication_fixture(&receipt),
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
    )
    .await;
    let runtime = match initialized {
        Ok(runtime) => runtime,
        Err(error) => {
            return Err(format!(
                "initialization failed: {error}; observed requests: {}",
                std::fs::read_to_string(&receipt)?
            )
            .into());
        }
    };

    let endpoint = endpoint("cursor-local")?;
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("provider-operations.sqlite")).await?,
    ));
    let backend = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: binding(endpoint.clone(), "cursor")?,
            runtime,
        }],
        store.clone(),
    )?;
    // Act: two sessions share this one initialized connection.
    let first = backend
        .open_provider_fixture(&endpoint, root.path().to_owned())
        .await;
    let second = if first.is_ok() {
        Some(
            backend
                .open_provider_fixture(&endpoint, root.path().to_owned())
                .await,
        )
    } else {
        None
    };
    let setting = if let Ok(session_id) = &first {
        let target = SessionRef {
            endpoint: endpoint.clone(),
            session_id: SessionId::try_from(session_id.clone())?,
        };
        let actor = target.clone().into();
        store
            .lock()
            .await
            .record_session(&collaboration_runtime::ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    root.path().display().to_string(),
                )?,
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 0,
                resume: Default::default(),
            })
            .await?;
        Some(
            backend
                .settings_set(ProviderSettingsSetRequest {
                    target,
                    actor,
                    setting: ProviderSettingName::Effort,
                    value: "high".to_owned(),
                })
                .await,
        )
    } else {
        None
    };
    backend.shutdown().await?;
    let process_id = rustix::process::Pid::from_raw(
        std::fs::read_to_string(receipt.with_extension("log.pid"))?.parse()?,
    )
    .ok_or("invalid fixture process ID")?;
    ensure_eq!(
        rustix::process::test_kill_process(process_id),
        Err(rustix::io::Errno::SRCH)
    );

    // Assert: no discovery or admission can overtake the required handshake.
    ensure_eq!(
        std::fs::read_to_string(&receipt)?,
        "initialize\nauthenticate\ncursor/list_available_models\nsession/new\nsession/new\nsession/set_config_option\n"
    );
    ensure_eq!(first?, "authenticated-session-1");
    ensure_eq!(
        second.ok_or("first session failed")??,
        "authenticated-session-2"
    );
    let result = setting
        .ok_or("first session failed")?
        .map_err(|error| format!("provider setting failed: {error:?}"))?;
    ensure_eq!(result.effective_settings.effort.as_deref(), Some("high"));
    Ok(())
}
