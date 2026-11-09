//! `host status` works out, when asked, that the Router executable was replaced after start.
//!
//! The Host runs from a signed copy of this test binary that the test owns, so the test can
//! replace that launch path the way an upgrade does without touching a shared binary.
use std::path::{Path, PathBuf};
use std::process::Stdio;
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
use codex_router_host::HostSnapshot;
use codex_router_host::ManagedChildLaunchPlans;
use codex_router_host::ManagedUpdateInputs;
use codex_router_host::OperatorFrame;
use codex_router_host::OperatorRequest;
use codex_router_host::RouterExecutableRelation;
use codex_router_test_support::native_app_server::run_native_app_server_fixture;
use codex_router_test_support::router_health::PersistentRouterHealthFixture;

use crate::host_replacement_cleanup::{
    finish_fixture_proof, reap_fixture_host, terminate_logged_fixture_processes,
};
use crate::operator_client::send_operator_request;

const RUNNING_VERSION: &str = "0.1.36";
const INSTALLED_VERSION: &str = "9.9.9";

type ProofResult = Result<(), Box<dyn std::error::Error>>;

/// Signs `binary` with the repository's debug identity and refuses it unless macOS reports
/// TeamIdentifier=974QD84WVC and a valid signature right before it is launched.
async fn sign_owned_copy(binary: &Path) -> ProofResult {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let signed =
        tokio::process::Command::new(repository.join("scripts/cargo_debug_signing_runner.sh"))
            .arg("--sign-only")
            .arg(binary)
            .kill_on_drop(true)
            .output()
            .await?;
    if !signed.status.success()
        || String::from_utf8_lossy(&signed.stderr).contains("stays linker-signed")
    {
        return Err(format!(
            "signing the owned Host copy failed: {}",
            String::from_utf8_lossy(&signed.stderr)
        )
        .into());
    }
    #[cfg(target_os = "macos")]
    {
        let signature = tokio::process::Command::new("codesign")
            .arg("-dv")
            .arg(binary)
            .output()
            .await?;
        let verified = tokio::process::Command::new("codesign")
            .arg("--verify")
            .arg(binary)
            .status()
            .await?;
        if !signature.status.success()
            || !String::from_utf8_lossy(&signature.stderr)
                .lines()
                .any(|line| line == "TeamIdentifier=974QD84WVC")
            || !verified.success()
        {
            return Err(format!(
                "refusing to run {}: it does not carry a verified TeamIdentifier=974QD84WVC",
                binary.display()
            )
            .into());
        }
    }
    Ok(())
}

/// Installs a new executable at `launch_path` the way an upgrade does: a new file renamed
/// over the old one. This one only answers `--version`.
fn replace_launch_path_atomically(launch_path: &Path) -> ProofResult {
    let replacement = launch_path.with_extension("replacement");
    std::fs::write(
        &replacement,
        format!("#!/bin/sh\nprintf 'codex-router {INSTALLED_VERSION}\\n'\n"),
    )?;
    std::fs::set_permissions(
        &replacement,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )?;
    std::fs::rename(replacement, launch_path)?;
    Ok(())
}

fn terminal_snapshot(
    frames: &[OperatorFrame],
) -> Result<&HostSnapshot, Box<dyn std::error::Error>> {
    frames
        .iter()
        .find_map(|frame| match frame {
            OperatorFrame::Terminal(response) => Some(response.snapshot()),
            OperatorFrame::Progress(_) => None,
        })
        .ok_or_else(|| "status answered without a terminal frame".into())
}

async fn status_router_relation(
    operator_socket: &Path,
) -> Result<RouterExecutableRelation, Box<dyn std::error::Error>> {
    let frames = send_operator_request(
        operator_socket,
        OperatorRequest::Status,
        Duration::from_secs(20),
    )
    .await?;
    Ok(terminal_snapshot(&frames)?
        .router_executable_relation()
        .clone())
}

async fn wait_for_operator_socket(
    operator_socket: &Path,
    host: &mut tokio::process::Child,
) -> ProofResult {
    (async {
        loop {
            if let Some(status) = host.try_wait()? {
                return Err(format!("the owned Host exited before serving: {status}").into());
            }
            if tokio::net::UnixStream::connect(operator_socket)
                .await
                .is_ok()
            {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

#[tokio::test]
async fn host_status_reports_a_router_replaced_since_start_with_no_timer() -> ProofResult {
    let directory = tempfile::tempdir()?;
    let router = PersistentRouterHealthFixture::start().await?;
    let operator_socket = directory.path().join("operator.sock");
    let app_server_log = directory.path().join("app-pids.log");
    std::fs::write(&app_server_log, b"")?;
    let launch_path = directory.path().join("codex-router");
    std::fs::copy(std::env::current_exe()?, &launch_path)?;
    sign_owned_copy(&launch_path).await?;
    let mut host = tokio::process::Command::new(&launch_path);
    host.args([
        "--exact",
        &crate::child_test_selector(module_path!(), "owned_copy_host_child_entrypoint"),
        "--nocapture",
    ])
    .env_remove(codex_router_host::inherited_lock_environment())
    .env("CODEX_ROUTER_DEBUG_RUNNING_VERSION", RUNNING_VERSION)
    .env("CODEX_HOST_STATUS_CHILD_DIRECTORY", directory.path())
    .env(
        "CODEX_HOST_STATUS_CHILD_ROUTER",
        router.address().to_string(),
    )
    .kill_on_drop(true)
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let mut host = host.spawn()?;

    let proof_result: ProofResult = async {
        wait_for_operator_socket(&operator_socket, &mut host).await?;
        let before = status_router_relation(&operator_socket).await?;

        replace_launch_path_atomically(&launch_path)?;
        let after = status_router_relation(&operator_socket).await?;

        if before != RouterExecutableRelation::Match {
            return Err(format!("status before the replacement: {before:?}").into());
        }
        let expected = RouterExecutableRelation::Drift {
            running_version: RUNNING_VERSION.to_owned(),
            installed_version: Some(INSTALLED_VERSION.to_owned()),
        };
        if after != expected {
            return Err(format!("status after the replacement: {after:?}").into());
        }
        Ok(())
    }
    .await;

    let host_cleanup = reap_fixture_host(&mut host).await;
    let child_cleanup = terminate_logged_fixture_processes(&[app_server_log.as_path()]);
    let router_cleanup = router.finish().await;
    finish_fixture_proof(proof_result, [host_cleanup, child_cleanup, router_cleanup])
}

/// The Host the parent observes. It runs from the parent's owned copy, so its launch path
/// is the file the parent replaces; it has no collaboration directory, so no provider starts.
#[tokio::test]
async fn owned_copy_host_child_entrypoint() -> ProofResult {
    let Some(directory) = std::env::var_os("CODEX_HOST_STATUS_CHILD_DIRECTORY") else {
        return Ok(());
    };
    let directory = PathBuf::from(directory);
    let router_endpoint = std::env::var("CODEX_HOST_STATUS_CHILD_ROUTER")?.parse()?;
    let launch_path = std::env::current_exe()?;
    let app_server_socket = directory.join("app.sock");
    let identity = codex_native_integration::executable_identity(&launch_path).await?;
    let app_server = AppServerLaunchPlan::new(
        ChildCommandSpec::new(launch_path)
            .with_arguments([
                "--exact",
                &crate::child_test_selector(
                    module_path!(),
                    "owned_copy_app_server_child_entrypoint",
                ),
                "--nocapture",
            ])
            .with_environment("CODEX_HOST_STATUS_APP_SOCKET", &app_server_socket)
            .with_environment("CODEX_HOST_STATUS_APP_LOG", directory.join("app-pids.log"))
            .with_output(ChildOutput::Null),
        identity,
        "1.2.3".to_owned(),
    );
    let result = Box::pin(HostRuntime::run(
        HostConfig::new(HostConfigInputs {
            coordination_paths: HostCoordinationPaths::new(
                directory.join("operator.sock"),
                directory.join("instance.lock"),
            ),
            router_endpoint,
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            app_server_socket,
            managed_executable: PathBuf::from("/unused/managed-codex"),
            deadlines: HostDeadlines::new(HostDeadlineInputs {
                router_start: Duration::from_secs(1),
                app_server_start: Duration::from_secs(5),
                remote_control: Duration::from_secs(1),
                endpoint_inspection: Duration::from_millis(200),
                operator_request: Duration::from_secs(15),
            })?,
        }),
        ManagedChildLaunchPlans::new(None, app_server),
        ManagedUpdateInputs::production(),
    ))
    .await;
    Err(format!("the owned Host returned while the parent still observed it: {result:?}").into())
}

#[tokio::test]
async fn owned_copy_app_server_child_entrypoint() -> ProofResult {
    let Some(socket_path) = std::env::var_os("CODEX_HOST_STATUS_APP_SOCKET") else {
        return Ok(());
    };
    let process_log = std::env::var_os("CODEX_HOST_STATUS_APP_LOG")
        .ok_or("the owned Host's app-server process log is missing")?;
    run_native_app_server_fixture(
        Path::new(&socket_path),
        "1.2.3",
        Some(Path::new(&process_log)),
    )
    .await
}
