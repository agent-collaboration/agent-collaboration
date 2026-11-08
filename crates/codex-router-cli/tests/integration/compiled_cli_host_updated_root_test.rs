//! J0 (never-connected root): the update gives a root that has never held a Fleet
//! database its collaboration directory and Fleet database, and a real signed Host then
//! starts on that root and reports itself ready.
use super::{
    TestDirectory, check, install_debug_profile, install_launchctl_fixture,
    install_managed_fixture, reserve_loopback_port, sign_debug_binary,
};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Bounds each wait on the Host's own signals, only so a Host that never gives one
/// fails the test: its readiness line, and its answers to `host status` and
/// `host stop`. Its collaboration runtime took about 16 seconds to come up on a loaded
/// development machine, with or without the update before it.
pub(super) const HOST_SIGNAL_BOUND: Duration = Duration::from_secs(60);

/// The readiness-timing stage the Host writes once its collaboration runtime is
/// synchronized; its operator socket is bound and `host status` answers after it.
const COLLABORATION_SYNCHRONIZED: &str = r#""stage":"collaborationSynchronized""#;

/// Reads the Host's stderr line by line, copying each line into `log`, until the Host
/// reports its collaboration runtime synchronized. A Host that exits first ends the
/// stream and fails the wait with what it wrote. The returned task keeps draining the
/// rest into `log`, so the Host never blocks on a full pipe.
pub(super) async fn host_synchronized(
    stderr: tokio::process::ChildStderr,
    log: &Path,
) -> Result<tokio::task::JoinHandle<std::io::Result<()>>, Box<dyn std::error::Error>> {
    let mut lines = BufReader::new(stderr).lines();
    let mut file = tokio::fs::File::create(log).await?;
    let synchronized = tokio::time::timeout(HOST_SIGNAL_BOUND, async {
        while let Some(line) = lines.next_line().await? {
            file.write_all(format!("{line}\n").as_bytes()).await?;
            if line.contains(COLLABORATION_SYNCHRONIZED) {
                return Ok(true);
            }
        }
        Ok::<bool, std::io::Error>(false)
    })
    .await??;
    file.flush().await?;
    if !synchronized {
        return Err(format!(
            "fixture Host exited before it was ready: {}",
            std::fs::read_to_string(log)?
        )
        .into());
    }
    Ok(tokio::spawn(async move {
        while let Some(line) = lines.next_line().await? {
            file.write_all(format!("{line}\n").as_bytes()).await?;
        }
        file.flush().await
    }))
}

/// Runs `codex-router host <subcommand>` against the fixture Host and returns its answer.
pub(super) async fn host_answer(
    binary: &Path,
    router_root: &Path,
    codex_home: &Path,
    app_server_socket: &Path,
    subcommand: &str,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let home = codex_home.parent().ok_or("fixture HOME is missing")?;
    Ok(tokio::time::timeout(
        HOST_SIGNAL_BOUND,
        tokio::process::Command::new(binary)
            .args(["host", subcommand, "--router-root"])
            .arg(router_root)
            .env("CODEX_ROUTER_USE_HOME_DEFAULT", "1")
            .env("OTEL_SDK_DISABLED", "true")
            .env("HOME", home)
            .env("CODEX_HOME", codex_home)
            .env("CODEX_ROUTER_COMPILED_CLI_NATIVE_SOCKET", app_server_socket)
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

#[tokio::test]
async fn a_host_starts_ready_on_a_never_connected_root_after_the_update() -> TestResult {
    // Arrange: an owner-private root with nothing in it, and the managed Codex fixture.
    let directory = TestDirectory::new()?;
    let router_root = directory.path().join("private-router");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&router_root)?;
    let codex_home = directory.path().join("codex");
    let socket_path = directory.path().join("dedicated-socket/app-server.sock");
    let managed_executable = codex_home.join("packages/standalone/current/codex");
    std::fs::create_dir_all(
        managed_executable
            .parent()
            .ok_or("managed executable parent is missing")?,
    )?;
    install_managed_fixture(&managed_executable)?;
    let launchctl_executable = directory.path().join("launchctl");
    install_launchctl_fixture(&launchctl_executable)?;
    let port = reserve_loopback_port()?;
    install_debug_profile(&codex_home, port)?;
    let binary = directory.path().join("codex-router");
    std::fs::copy(env!("CARGO_BIN_EXE_codex-router"), &binary)?;
    sign_debug_binary(&binary).await?;

    // Act: the update, then a real Host on the same root.
    agent_router_update::run_router_update(&router_root).await?;
    let collaboration = router_root.join("agent-communication");
    let mode = std::fs::metadata(&collaboration)?.permissions().mode();
    let host_stderr = directory.path().join("host-stderr.log");
    let mut host = tokio::process::Command::new(&binary)
        .args(["host", "--require-debug-isolation", "--router-root"])
        .arg(&router_root)
        .args(["--port", &port.to_string(), "--mcp-bind", "127.0.0.1:0"])
        .env("CODEX_ROUTER_USE_HOME_DEFAULT", "1")
        .env("OTEL_SDK_DISABLED", "true")
        .env("HOME", directory.path())
        .env("CODEX_HOME", &codex_home)
        .env("CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET", &socket_path)
        .env("CODEX_ROUTER_COMPILED_CLI_NATIVE_SOCKET", &socket_path)
        .env("CODEX_ROUTER_DEBUG_LAUNCHCTL", &launchctl_executable)
        .env(
            "CODEX_ROUTER_COMPILED_CLI_LAUNCHCTL_LOG",
            directory.path().join("launchctl.log"),
        )
        .env(
            "CODEX_ROUTER_COMPILED_CLI_TEST_BINARY",
            std::env::current_exe()?,
        )
        .env("CODEX_ROUTER_COMPILED_CLI_APP_CHILD", "1")
        // Each app-server stand-in records its pid here, so the directory's cleanup
        // stops it even when this Host is killed instead of stopped.
        .env(
            "CODEX_ROUTER_COMPILED_CLI_PROCESS_LOG",
            directory.path().join("app-generations.log"),
        )
        .env("CODEX_ROUTER_COMPILED_CLI_MANAGED", &managed_executable)
        .env("CODEX_ROUTER_DEBUG_READINESS_TIMING", "1")
        // A fresh Host, not a handoff: it must not inherit a running Host's lock marker.
        .env_remove(codex_router_host::inherited_lock_environment())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = host.stderr.take().ok_or("Host stderr unavailable")?;
    let ready = async {
        let drained = host_synchronized(stderr, &host_stderr).await?;
        let status =
            host_answer(&binary, &router_root, &codex_home, &socket_path, "status").await?;
        let stdout = String::from_utf8(status.stdout)?;
        check(status.status.success(), &stdout)?;
        check(stdout.contains("readiness: ready"), &stdout)?;
        Ok::<_, Box<dyn std::error::Error>>(drained)
    }
    .await;
    let stopped = host_answer(&binary, &router_root, &codex_home, &socket_path, "stop").await;
    // A Host that doesn't exit after `stop` is killed here, which also ends its stderr,
    // so neither this wait nor the drain below can outlast its bound.
    let Ok(exited) = tokio::time::timeout(HOST_SIGNAL_BOUND, host.wait()).await else {
        host.kill().await?;
        return Err(format!(
            "the Host did not exit within {HOST_SIGNAL_BOUND:?} of `host stop`; stderr: {}",
            std::fs::read_to_string(&host_stderr).unwrap_or_default()
        )
        .into());
    };

    // Assert
    let drained = ready.map_err(|error| {
        std::io::Error::other(format!(
            "{error}; fixture Host stderr: {}",
            std::fs::read_to_string(&host_stderr).unwrap_or_default()
        ))
    })?;
    tokio::time::timeout(HOST_SIGNAL_BOUND, drained).await???;
    check(
        mode & 0o077 == 0,
        &format!("the update made the collaboration directory {mode:o}, not owner-private"),
    )?;
    check(stopped?.status.success(), "the Host must stop cleanly")?;
    check(exited?.success(), "the Host must exit cleanly")
}
