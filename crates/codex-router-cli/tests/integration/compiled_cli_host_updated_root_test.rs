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

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Bounds each wait on the Host's own signals: its operator socket, and its answers to
/// `host status` and `host stop`. The status answer comes once its collaboration runtime
/// is up, which took about 16 seconds on a loaded development machine, with or without
/// the update before it.
const HOST_SIGNAL_BOUND: Duration = Duration::from_secs(60);

/// Waits until the Host binds its operator socket, failing at once if it exits first.
async fn operator_socket_bound(
    host: &mut tokio::process::Child,
    socket: &Path,
    stderr: &Path,
) -> TestResult {
    tokio::time::timeout(HOST_SIGNAL_BOUND, async {
        while !socket.exists() {
            if let Some(status) = host.try_wait()? {
                return Err(std::io::Error::other(format!(
                    "fixture Host exited {status}: {}",
                    std::fs::read_to_string(stderr)?
                )));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

/// Runs `codex-router host <subcommand>` against the fixture Host and returns its answer.
async fn host_answer(
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
        .env("CODEX_ROUTER_COMPILED_CLI_MANAGED", &managed_executable)
        .env("CODEX_ROUTER_DEBUG_READINESS_TIMING", "1")
        // A fresh Host, not a handoff: it must not inherit a running Host's lock marker.
        .env_remove(codex_router_host::inherited_lock_environment())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&host_stderr)?))
        .kill_on_drop(true)
        .spawn()?;
    let ready = async {
        operator_socket_bound(&mut host, &router_root.join("host.sock"), &host_stderr).await?;
        let status =
            host_answer(&binary, &router_root, &codex_home, &socket_path, "status").await?;
        let stdout = String::from_utf8(status.stdout)?;
        check(status.status.success(), &stdout)?;
        check(stdout.contains("readiness: ready"), &stdout)
    }
    .await;
    let stopped = host_answer(&binary, &router_root, &codex_home, &socket_path, "stop").await;
    let exited = tokio::time::timeout(HOST_SIGNAL_BOUND, host.wait()).await;

    // Assert
    ready.map_err(|error| {
        std::io::Error::other(format!(
            "{error}; fixture Host stderr: {}",
            std::fs::read_to_string(&host_stderr).unwrap_or_default()
        ))
    })?;
    check(
        mode & 0o077 == 0,
        &format!("the update made the collaboration directory {mode:o}, not owner-private"),
    )?;
    check(stopped?.status.success(), "the Host must stop cleanly")?;
    check(exited??.success(), "the Host must exit cleanly")
}
