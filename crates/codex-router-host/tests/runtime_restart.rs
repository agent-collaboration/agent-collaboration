use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_router_host::AppServerLaunchPlan;
use codex_router_host::ChildCommandSpec;
use codex_router_host::ChildOutput;
use codex_router_host::HostConfig;
use codex_router_host::HostConfigInputs;
use codex_router_host::HostCoordinationPaths;
use codex_router_host::HostDeadlineInputs;
use codex_router_host::HostDeadlines;
use codex_router_host::HostRuntime;
use codex_router_host::ManagedChildLaunchPlans;
use codex_router_host::ManagedUpdateInputs;
use codex_router_host::OperatorFrame;
use codex_router_host::OperatorRequest;
use codex_router_host::RecoveryBudget;
use codex_router_host::RouterCondition;
use codex_router_host::TerminalClassification;
use codex_router_test_support::native_app_server::run_native_app_server_fixture;
use codex_router_test_support::router_health::run_persistent_router_health_fixture;

#[path = "support/operator_client.rs"]
mod operator_client;
use operator_client::send_operator_request;

#[path = "support/restart_fixture_lifetime.rs"]
mod restart_fixture_lifetime;
use restart_fixture_lifetime::{FIXTURE_SCOPE, REGISTRY_ENVIRONMENT, RestartFixtureLifetime};
use restart_fixture_lifetime::{register_fixture_child, stop_and_reap_process};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn owned_router_restart_replaces_only_router_and_preserves_app_server_state()
-> Result<(), Box<dyn std::error::Error>> {
    let _fixture_scope = FIXTURE_SCOPE.lock().await;
    run_restart_scenario(None).await
}

struct StartedFixture {
    process_ids: [u32; 2],
    router_address: SocketAddr,
}

enum FixtureExit<'a> {
    Error(&'a mut Option<StartedFixture>),
    Cancelled(tokio::sync::oneshot::Sender<StartedFixture>),
}

#[tokio::test]
async fn early_error_stops_and_reaps_restart_fixture_children()
-> Result<(), Box<dyn std::error::Error>> {
    let _fixture_scope = FIXTURE_SCOPE.lock().await;
    // Arrange: reach real Host readiness, then leave the scenario through an error.
    let mut started = None;
    let result = run_restart_scenario(Some(FixtureExit::Error(&mut started))).await;
    check(
        result.is_err_and(|error| error.to_string() == "forced early fixture error"),
        "scenario must reach the deliberately injected early error",
    )?;
    let started = started.ok_or("fixture did not publish its started children")?;

    require_fixture_reaped(started)
}

#[tokio::test]
async fn cancelled_scenario_stops_and_reaps_restart_fixture_children()
-> Result<(), Box<dyn std::error::Error>> {
    let _fixture_scope = FIXTURE_SCOPE.lock().await;
    let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
    let mut scenario = Box::pin(run_restart_scenario(Some(FixtureExit::Cancelled(
        started_sender,
    ))));
    let started = tokio::select! {
        started = started_receiver => started?,
        _result = &mut scenario => return Err("scenario ended before cancellation".into()),
    };
    // Cancelling the fixture future must run its Drop guard, without finish().
    drop(scenario);
    require_fixture_reaped(started)
}

fn require_fixture_reaped(started: StartedFixture) -> Result<(), Box<dyn std::error::Error>> {
    // Observe before the fallback cleanup, so that cleanup cannot hide the defect.
    let children_absent = started.process_ids.iter().all(|process_id| {
        let process_id = i32::try_from(*process_id)
            .ok()
            .and_then(rustix::process::Pid::from_raw);
        matches!(
            process_id.map(rustix::process::test_kill_process),
            Some(Err(rustix::io::Errno::SRCH))
        )
    });
    let port_released = std::net::TcpListener::bind(started.router_address).is_ok();
    for process_id in started.process_ids {
        stop_and_reap_process(process_id)?;
    }
    check(
        children_absent,
        "early error left a live or unreaped fixture child",
    )?;
    check(port_released, "early error left the fixture port listening")
}

async fn run_restart_scenario(
    early_exit: Option<FixtureExit<'_>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _diagnostics = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::INFO)
            .finish(),
    );
    let directory = TestDirectory::new("owned-router-restart")?;
    let registry_path = directory.path().join("child-registry");
    let mut lifetime = RestartFixtureLifetime::new(registry_path.clone())?;
    let router_probe = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let router_address = router_probe.local_addr()?;
    drop(router_probe);
    let router_process_log = directory.path().join("router-pids.log");
    let app_server_process_log = directory.path().join("app-server-pids.log");
    std::fs::write(&router_process_log, b"")?;
    std::fs::write(&app_server_process_log, b"")?;

    let current_executable = std::env::current_exe()?;
    let app_server_socket = directory.path().join("app.sock");
    let app_server_socket_for_gap = app_server_socket.clone();
    let managed_executable = directory.path().join("managed-codex");
    std::fs::write(
        &managed_executable,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; else exec '{}' \"$@\"; fi\n",
            current_executable.display()
        ),
    )?;
    std::fs::set_permissions(
        &managed_executable,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let identity = codex_native_integration::executable_identity(&current_executable).await?;
    let app_server = AppServerLaunchPlan::new(
        ChildCommandSpec::new(current_executable.clone())
            .with_arguments([
                "--exact",
                "owned_router_restart_app_server_child_entrypoint",
                "--nocapture",
            ])
            .with_environment("CODEX_HOST_RESTART_APP_SOCKET", &app_server_socket)
            .with_environment("CODEX_HOST_RESTART_APP_LOG", &app_server_process_log)
            .with_environment("CODEX_FIXTURE_IGNORE_TERM", "1")
            .with_environment(REGISTRY_ENVIRONMENT, &registry_path)
            .with_output(ChildOutput::Null),
        identity,
        "1.2.3".to_owned(),
    );
    let router_command = ChildCommandSpec::new(current_executable)
        .with_arguments([
            "--exact",
            "owned_router_restart_child_entrypoint",
            "--nocapture",
        ])
        .with_environment(
            "CODEX_HOST_RESTART_ROUTER_ADDRESS",
            router_address.to_string(),
        )
        .with_environment("CODEX_HOST_RESTART_ROUTER_LOG", &router_process_log)
        .with_environment(REGISTRY_ENVIRONMENT, &registry_path)
        .with_output(ChildOutput::Null);
    let coordination_paths = HostCoordinationPaths::new(
        directory.path().join("operator.sock"),
        directory.path().join("instance.lock"),
    );
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: coordination_paths.clone(),
        router_endpoint: router_address,
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        app_server_socket,
        managed_executable,
        deadlines: HostDeadlines::new(HostDeadlineInputs {
            router_start: Duration::from_millis(500),
            app_server_start: Duration::from_secs(2),
            remote_control: Duration::from_secs(1),
            endpoint_inspection: Duration::from_millis(200),
            operator_request: Duration::from_secs(3),
        })?,
    });
    lifetime.attach(tokio::spawn(HostRuntime::run(
        config,
        ManagedChildLaunchPlans::new(Some(router_command), app_server),
        ManagedUpdateInputs::production(),
    )));

    let scenario_result = async {
        let startup = send_operator_request(
            coordination_paths.operator_socket(),
            OperatorRequest::AwaitHostStart,
            Duration::from_secs(4),
        )
        .await?;
        check_equal(
            terminal_classification(&startup)?,
            TerminalClassification::Ready,
            "owned-router host startup must become ready",
        )?;
        let initial_app_server_pid = *wait_for_process_ids(&app_server_process_log, 1)
            .await?
            .first()
            .ok_or("initial app-server PID is missing")?;
        let initial_router_pid = *wait_for_process_ids(&router_process_log, 1)
            .await?
            .first()
            .ok_or("initial router PID is missing")?;
        if let Some(exit) = early_exit {
            let started = StartedFixture {
                process_ids: [initial_router_pid, initial_app_server_pid],
                router_address,
            };
            match exit {
                FixtureExit::Error(observed) => {
                    *observed = Some(started);
                    return Err("forced early fixture error".into());
                }
                FixtureExit::Cancelled(sender) => {
                    sender
                        .send(started)
                        .map_err(|_| "cancellation observer closed")?;
                    std::future::pending::<()>().await;
                }
            }
        }

        let restart = send_operator_request(
            coordination_paths.operator_socket(),
            OperatorRequest::RestartRouter,
            Duration::from_secs(4),
        )
        .await?;
        check_equal(
            terminal_classification(&restart)?,
            TerminalClassification::Succeeded,
            "owned router restart must succeed",
        )?;
        check(
            restart.iter().any(|frame| {
                matches!(
                    frame,
                    OperatorFrame::Progress(codex_router_host::HostProgress::StoppingRouter)
                )
            }) && restart.iter().any(|frame| {
                matches!(
                    frame,
                    OperatorFrame::Progress(codex_router_host::HostProgress::RouterReady)
                )
            }),
            "router restart must stream stopping and ready phases",
        )?;
        let restarted_router_pids = wait_for_process_ids(&router_process_log, 2).await?;
        let restarted_router_pid = *restarted_router_pids
            .get(1)
            .ok_or("restarted router PID is missing")?;
        check(
            restarted_router_pid != initial_router_pid,
            "owned router restart must replace the exact router child",
        )?;
        check_equal(
            *wait_for_process_ids(&app_server_process_log, 1)
                .await?
                .first()
                .ok_or("preserved app-server PID is missing")?,
            initial_app_server_pid,
            "router restart must not replace the app-server",
        )?;
        check_equal(
            terminal_snapshot(&restart)?.recovery_budget(),
            RecoveryBudget::Available,
            "router restart must not consume app-server recovery",
        )?;

        terminate_process(restarted_router_pid)?;
        let unavailable = wait_for_router_condition(
            coordination_paths.operator_socket(),
            RouterCondition::Unavailable,
        )
        .await?;
        let unavailable_snapshot = terminal_snapshot(&unavailable)?;
        check(
            unavailable_snapshot.app_server()
                == &codex_router_host::AppServerCondition::NativeReady {
                    running_version: "1.2.3".to_owned(),
                },
            &format!(
                "owned-router exit must preserve the healthy app-server: {:?}",
                unavailable_snapshot.app_server()
            ),
        )?;
        let recovery_restart = send_operator_request(
            coordination_paths.operator_socket(),
            OperatorRequest::RestartRouter,
            Duration::from_secs(4),
        )
        .await?;
        check_equal(
            terminal_classification(&recovery_restart)?,
            TerminalClassification::Succeeded,
            "explicit router restart must recover an owned unavailable router",
        )?;
        let _recovered_router_pids = wait_for_process_ids(&router_process_log, 3).await?;

        let restart_started_at = tokio::time::Instant::now();
        let operator_socket_for_restart = coordination_paths.operator_socket().to_owned();
        let restart_future = send_operator_request(
            &operator_socket_for_restart,
            OperatorRequest::RestartAppServer,
            Duration::from_secs(4),
        );
        let (restart_result, restarted_app_server_pids) = tokio::join!(
            restart_future,
            wait_for_process_ids(&app_server_process_log, 2)
        );
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if tokio::net::UnixStream::connect(&app_server_socket_for_gap)
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let restart_gap = restart_started_at.elapsed();
        eprintln!("app_server_restart_gap_ms={}", restart_gap.as_millis());
        check(
            restart_gap < Duration::from_secs(5),
            &format!("app-server restart socket gap exceeded target: {restart_gap:?}"),
        )?;
        let restart_result = restart_result?;
        check(
            terminal_classification(&restart_result)? == TerminalClassification::Succeeded,
            &format!("app-server restart must succeed: {restart_result:?}"),
        )?;
        check(
            restart_result.iter().any(|frame| {
                matches!(
                    frame,
                    OperatorFrame::Progress(codex_router_host::HostProgress::StoppingAppServer)
                )
            }) && restart_result.iter().any(|frame| {
                matches!(
                    frame,
                    OperatorFrame::Progress(codex_router_host::HostProgress::AppServerReady)
                )
            }) && restart_result.iter().any(|frame| {
                matches!(
                    frame,
                    OperatorFrame::Progress(codex_router_host::HostProgress::AppServerKilled)
                )
            }),
            "app-server restart must stream stopping and ready phases",
        )?;
        let _restarted_app_server_pid = restarted_app_server_pids?
            .get(1)
            .copied()
            .ok_or("restarted app-server PID is missing")?;
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    let cleanup_result = lifetime.finish().await;
    match (scenario_result, cleanup_result) {
        (Err(error), Err(cleanup)) => {
            Err(std::io::Error::other(format!("{error}; cleanup: {cleanup}")).into())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), result) => Ok(result?),
    }
}

async fn wait_for_router_condition(
    operator_socket: &Path,
    expected: RouterCondition,
) -> Result<Vec<OperatorFrame>, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Ok(frames) = send_operator_request(
                operator_socket,
                OperatorRequest::Status,
                Duration::from_secs(1),
            )
            .await
                && terminal_snapshot(&frames).is_ok_and(|snapshot| snapshot.router() == expected)
            {
                return frames;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?)
}

#[tokio::test]
async fn owned_router_restart_child_entrypoint() -> Result<(), Box<dyn std::error::Error>> {
    let Some(address) = std::env::var_os("CODEX_HOST_RESTART_ROUTER_ADDRESS") else {
        return Ok(());
    };
    let process_log = std::env::var_os("CODEX_HOST_RESTART_ROUTER_LOG")
        .ok_or("owned router process log is missing")?;
    register_fixture_child()?;
    run_persistent_router_health_fixture(
        address.to_string_lossy().parse::<SocketAddr>()?,
        Path::new(&process_log),
    )
    .await
}

#[tokio::test]
async fn owned_router_restart_app_server_child_entrypoint() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(socket_path) = std::env::var_os("CODEX_HOST_RESTART_APP_SOCKET") else {
        return Ok(());
    };
    let process_log = std::env::var_os("CODEX_HOST_RESTART_APP_LOG")
        .ok_or("owned restart app-server process log is missing")?;
    register_fixture_child()?;
    run_native_app_server_fixture(
        Path::new(&socket_path),
        "1.2.3",
        Some(Path::new(&process_log)),
    )
    .await
}

fn terminal_classification(
    frames: &[OperatorFrame],
) -> Result<TerminalClassification, Box<dyn std::error::Error>> {
    frames
        .iter()
        .find_map(|frame| match frame {
            OperatorFrame::Terminal(response) => Some(response.classification()),
            OperatorFrame::Progress(_) => None,
        })
        .ok_or_else(|| std::io::Error::other("operator terminal response is missing").into())
}

fn terminal_snapshot(
    frames: &[OperatorFrame],
) -> Result<&codex_router_host::HostSnapshot, Box<dyn std::error::Error>> {
    frames
        .iter()
        .find_map(|frame| match frame {
            OperatorFrame::Terminal(response) => Some(response.snapshot()),
            OperatorFrame::Progress(_) => None,
        })
        .ok_or_else(|| std::io::Error::other("operator terminal response is missing").into())
}

async fn wait_for_process_ids(
    process_log: &Path,
    expected_count: usize,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let process_ids = std::fs::read_to_string(process_log)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.parse::<u32>().ok())
                .collect::<Vec<_>>();
            if process_ids.len() >= expected_count {
                return process_ids;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?)
}

fn terminate_process(process_id: u32) -> Result<(), Box<dyn std::error::Error>> {
    let process_id = rustix::process::Pid::from_raw(i32::try_from(process_id)?)
        .ok_or("fixture process ID must be nonzero")?;
    match rustix::process::kill_process(process_id, rustix::process::Signal::TERM) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn check_equal<TValue>(
    actual: TValue,
    expected: TValue,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    TValue: PartialEq,
{
    if actual == expected {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

fn check(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> std::io::Result<Self> {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("crhr-{name}-{}-{counter}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _cleanup_result = std::fs::remove_dir_all(&self.path);
    }
}
