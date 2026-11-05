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

    // Act: two sessions share this one initialized connection.
    let first = runtime.create_session(root.path().to_path_buf()).await;
    let second = if first.is_ok() {
        Some(runtime.create_session(root.path().to_path_buf()).await)
    } else {
        None
    };
    let setting = if let Ok(session_id) = &first {
        Some(
            runtime
                .set_setting(
                    session_id.clone(),
                    acp_client_runtime::ProviderSettingRequest {
                        kind: acp_client_runtime::ProviderSettingKind::Effort,
                        value: "high".to_owned(),
                        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                        expected_model: None,
                    },
                )
                .await,
        )
    } else {
        None
    };
    runtime.shutdown().await;
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
    let effective_setting = setting.ok_or("first session failed")??;
    ensure_eq!(effective_setting.effort.as_deref(), Some("high"));
    Ok(())
}
