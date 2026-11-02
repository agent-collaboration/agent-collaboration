//! Initialization barriers and cleanup through the existing runtime's timed entry point.

use super::*;
use futures_util::FutureExt as _;
use tokio::io::AsyncWriteExt as _;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const STARTUP_BOUND: Duration = Duration::from_secs(5);

async fn advance_initialization_clock(budget: Duration) {
    // The real authentication request is observed first. Resume before OS child
    // cleanup so idle Tokio time cannot auto-advance while waiting for real IO.
    tokio::time::pause();
    tokio::time::advance(budget).await;
    tokio::time::resume();
}

fn require_request_order(root: &std::path::Path, expected: &str) -> TestResult {
    let actual = std::fs::read_to_string(root.join("requests.log"))?;
    if actual != expected {
        return Err(
            format!("request order differed: expected={expected:?}, actual={actual:?}").into(),
        );
    }
    Ok(())
}

fn require_fixture_process_settled(root: &std::path::Path) -> TestResult {
    let process_id = rustix::process::Pid::from_raw(
        std::fs::read_to_string(root.join("requests.log.pid"))?.parse()?,
    )
    .ok_or("invalid fixture process ID")?;
    match rustix::process::test_kill_process(process_id) {
        Err(rustix::io::Errno::SRCH) => Ok(()),
        other => {
            Err(format!("owned fixture remains observable after settlement: {other:?}").into())
        }
    }
}

fn launch_authentication_fixture(root: &std::path::Path, scenario: &str) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: PathBuf::from("node"),
        arguments: vec![
            node_test_fixture::fixture_program("supervisor-integration-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "initialization-authentication".to_owned(),
            root.join("requests.log").to_string_lossy().into_owned(),
            scenario.to_owned(),
            root.join("release.sock").to_string_lossy().into_owned(),
        ],
        environment: vec![],
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    }
}

async fn exercise_sessions_and_setting(
    runtime: &ExternalProviderRuntime,
    root: &std::path::Path,
) -> TestResult {
    let first = runtime.create_session(root.to_path_buf()).await?;
    let second = runtime.create_session(root.to_path_buf()).await?;
    if first != "authenticated-session-1" || second != "authenticated-session-2" {
        return Err(format!("unexpected Session identities: {first:?}, {second:?}").into());
    }
    let settings = runtime
        .set_setting(
            first,
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Effort,
                value: "high".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await?;
    if settings.effort.as_deref() != Some("high") {
        return Err(format!("selected effort was not confirmed: {settings:?}").into());
    }
    Ok(())
}

/// The fixture's existing socketSignal rejects any incoming request while held.
/// Accepting that socket is a causal acknowledgement, not a delay-based absence check.
#[tokio::test]
async fn initialization_authentication_holds_discovery_and_readiness_until_the_reply() -> TestResult
{
    let root = tempfile::tempdir()?;
    let release = tokio::net::UnixListener::bind(root.path().join("release.sock"))?;
    let initializing = ExternalProviderRuntime::initialize_with_timeout(
        launch_authentication_fixture(root.path(), "held"),
        STARTUP_BOUND,
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
    );
    tokio::pin!(initializing);
    let accepted = tokio::select! {
        result = &mut initializing => {
            if let Ok(runtime) = result { runtime.shutdown().await; }
            return Err("initialization completed before its held authentication reply".into());
        }
        accepted = tokio::time::timeout(STARTUP_BOUND, release.accept()) => accepted,
    };
    let (mut signal, _) = match accepted {
        Ok(Ok(accepted)) => accepted,
        failure => {
            if let Ok(runtime) = initializing.await {
                runtime.shutdown().await;
            }
            return Err(format!("authentication gate was not reached: {failure:?}").into());
        }
    };
    // Poll the actual constructor, not a JoinHandle whose scheduling could hide ready_tx.
    if let Some(result) = initializing.as_mut().now_or_never() {
        if let Ok(runtime) = result {
            runtime.shutdown().await;
        }
        return Err("readiness escaped the authentication barrier".into());
    }
    let held_requests = std::fs::read_to_string(root.path().join("requests.log"))?;
    signal.write_all(b"release").await?;
    let runtime = initializing.await?;
    let exercised = exercise_sessions_and_setting(&runtime, root.path()).await;
    runtime.shutdown().await;
    require_fixture_process_settled(root.path())?;
    exercised?;
    if held_requests != "initialize\nauthenticate\n" {
        return Err(format!("request escaped the held reply: {held_requests:?}").into());
    }
    require_request_order(
        root.path(),
        "initialize\nauthenticate\ncursor/list_available_models\nsession/new\nsession/new\nsession/set_config_option\n",
    )?;
    Ok(())
}

#[tokio::test]
async fn initialization_authentication_rejection_malformed_reply_and_connection_close_never_admit()
-> TestResult {
    for scenario in ["reject", "malformed", "close"] {
        let root = tempfile::tempdir()?;
        let initialized = ExternalProviderRuntime::initialize_with_timeout(
            launch_authentication_fixture(root.path(), scenario),
            STARTUP_BOUND,
            Arc::new(acp_provider_adapters::CursorAcpBehavior),
        )
        .await;
        if let Ok(runtime) = &initialized {
            runtime.shutdown().await;
        }
        require_fixture_process_settled(root.path())?;
        if initialized.is_ok() {
            return Err(format!("{scenario} was admitted").into());
        }
        if scenario != "close"
            && !matches!(&initialized,
            Err(ExternalProviderRuntimeError::Initialize(message))
                if message.contains("method=authenticate") && !message.contains("authentication required"))
        {
            return Err(format!(
                "{scenario} did not retain its sanitized authentication stage: {initialized:?}"
            )
            .into());
        }
        require_request_order(root.path(), "initialize\nauthenticate\n")?;
    }
    Ok(())
}

#[tokio::test]
async fn initialization_authentication_deadline_cancels_and_settles_the_owned_connection()
-> TestResult {
    let root = tempfile::tempdir()?;
    let release = tokio::net::UnixListener::bind(root.path().join("release.sock"))?;
    // This virtual budget never waits out a real clock. The distinct five-second
    // bounds below constrain only arrival and owned-process settlement.
    let initialization_budget = Duration::from_secs(60);
    let initializing = ExternalProviderRuntime::initialize_with_timeout(
        launch_authentication_fixture(root.path(), "timeout"),
        initialization_budget,
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
    );
    tokio::pin!(initializing);
    let accepted = tokio::select! {
        result = &mut initializing => {
            if let Ok(runtime) = result { runtime.shutdown().await; }
            return Err("initialization settled before authentication was observed".into());
        }
        accepted = tokio::time::timeout(STARTUP_BOUND, release.accept()) => accepted,
    };
    let _held_connection = match accepted {
        Ok(Ok((connection, _))) => connection,
        failure => {
            advance_initialization_clock(initialization_budget).await;
            if let Ok(Ok(runtime)) = tokio::time::timeout(STARTUP_BOUND, initializing).await {
                runtime.shutdown().await;
            }
            return Err(format!("authentication expiry gate was not reached: {failure:?}").into());
        }
    };
    require_request_order(root.path(), "initialize\nauthenticate\n")?;
    if let Some(result) = initializing.as_mut().now_or_never() {
        if let Ok(runtime) = result {
            runtime.shutdown().await;
        }
        return Err("initialization settled before the clock was advanced".into());
    }
    advance_initialization_clock(initialization_budget).await;
    let initialized = tokio::time::timeout(STARTUP_BOUND, initializing).await?;
    if let Ok(runtime) = &initialized {
        runtime.shutdown().await;
    }
    require_fixture_process_settled(root.path())?;
    if !matches!(
        initialized,
        Err(ExternalProviderRuntimeError::InitializeTimeout)
    ) {
        return Err(format!("expected the shared startup deadline: {initialized:?}").into());
    }
    require_request_order(root.path(), "initialize\nauthenticate\n")?;
    Ok(())
}

#[tokio::test]
async fn initialization_authentication_not_requested_preserves_claude_and_standard_connections()
-> TestResult {
    let policies: [Arc<dyn acp_client_runtime::ProviderBehavior>; 2] = [
        Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
            acp_provider_adapters::ClaudeSkillSource::Annotated,
        )),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    ];
    for policy in policies {
        let root = tempfile::tempdir()?;
        let runtime = ExternalProviderRuntime::initialize_with_timeout(
            launch_authentication_fixture(root.path(), "not-requested"),
            STARTUP_BOUND,
            policy,
        )
        .await?;
        let exercised = exercise_sessions_and_setting(&runtime, root.path()).await;
        runtime.shutdown().await;
        require_fixture_process_settled(root.path())?;
        exercised?;
        require_request_order(
            root.path(),
            "initialize\nsession/new\nsession/new\nsession/set_config_option\n",
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn initialization_authentication_is_required_again_on_a_replacement_connection() -> TestResult
{
    for _connection in 0..2 {
        let root = tempfile::tempdir()?;
        let runtime = ExternalProviderRuntime::initialize_with_timeout(
            launch_authentication_fixture(root.path(), "success"),
            STARTUP_BOUND,
            Arc::new(acp_provider_adapters::CursorAcpBehavior),
        )
        .await?;
        let exercised = exercise_sessions_and_setting(&runtime, root.path()).await;
        runtime.shutdown().await;
        require_fixture_process_settled(root.path())?;
        exercised?;
        require_request_order(
            root.path(),
            "initialize\nauthenticate\ncursor/list_available_models\nsession/new\nsession/new\nsession/set_config_option\n",
        )?;
    }
    Ok(())
}
