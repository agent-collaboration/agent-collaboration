//! J0 (whole root, real Host): a genuinely enrolled older Router root, holding every
//! file PR A's steps handle, is updated, and a real signed Host then starts on that same
//! root, takes its ports from the carried settings and reports itself ready.
//!
//! The real `enroll` command against a test central writes the identity files, the
//! enrollment, its secrets and the pending reset exactly as a Router does. The older
//! layout is then added beside them: the populated Fleet stores, the old settings files,
//! the old provider proxy store and a dead file.
use super::compiled_cli_host_fixture::{kill_fixture_processes, sign_debug_binary};
use super::reserve_loopback_port;
use super::updated_root_tests::{HOST_SIGNAL_BOUND, host_answer, host_synchronized};
use super::{check, install_debug_profile, install_launchctl_fixture, install_managed_fixture};
use crate::fleet_enrollment_producer::{
    create_and_read_project, fleet_cli, fleet_fixture::ProducerFixture,
};
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    sqlite::AsyncSqliteStateStore,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The files PR A must leave as `enroll` wrote them, relative to the Router root.
const KEPT_FOR_PR_B: [&str; 3] = [
    "agent-communication/service-identity.json",
    "agent-communication/installation-peer.json",
    "fleet/enrollment.json",
];

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Each kept file's bytes and permission bits.
fn kept_files(router_root: &Path) -> TestResult<Vec<(PathBuf, Vec<u8>, u32)>> {
    KEPT_FOR_PR_B
        .iter()
        .map(|name| {
            let path = router_root.join(name);
            let mode = std::fs::symlink_metadata(&path)?.permissions().mode() & 0o777;
            Ok((path.clone(), std::fs::read(&path)?, mode))
        })
        .collect()
}

/// Adds what an older Router leaves beside its enrollment: the populated Fleet stores,
/// `providers.json`, `ports.json` with `ports`, `routers.jsonc`, the provider proxy store
/// under its former name with one account, and a dead file.
async fn add_older_layout(router_root: &Path, ports: [u16; 3]) -> TestResult<AccountRecord> {
    let collaboration = router_root.join("agent-communication");
    let fixture = repository()
        .join("crates/agent-router-update/tests/fixtures/populated-root/agent-communication");
    for entry in std::fs::read_dir(&fixture)? {
        let entry = entry?;
        std::fs::copy(entry.path(), collaboration.join(entry.file_name()))?;
    }
    std::fs::write(
        router_root.join("providers.json"),
        "{\n  \"version\": 1,\n  \"providers\": {\n    \"claude\": {\n      \"enabled\": false,\n      \"executable\": null,\n      \"arguments\": []\n    },\n    \"cursor\": {\n      \"enabled\": false,\n      \"executable\": null,\n      \"arguments\": []\n    }\n  }\n}\n",
    )?;
    let [router, mcp, tailscale_https] = ports;
    std::fs::write(
        router_root.join("ports.json"),
        format!(
            "{{\"router\": {router}, \"mcp\": {mcp}, \"tailscaleHttps\": {tailscale_https}}}\n"
        ),
    )?;
    std::fs::write(
        router_root.join("routers.jsonc"),
        "{\n  // machines I open sessions on\n  \"version\": 1,\n  \"routers\": [],\n}\n",
    )?;
    let proxy = AsyncSqliteStateStore::open(&router_root.join("state.sqlite")).await?;
    let account = AccountRecord::new(
        Provider::Openai,
        AccountId::new("acct-whole-update")?,
        "Whole update proof",
        AccountStatus::Enabled,
    );
    proxy.upsert_account(&account).await?;
    proxy.close().await?;
    std::fs::write(
        collaboration.join("approval-history.json"),
        b"{\"older\": \"approval history\"}",
    )?;
    Ok(account)
}

/// The kept files keep their bytes and modes, and their owning readers accept them.
fn assert_kept_for_pr_b(router_root: &Path, before: &[(PathBuf, Vec<u8>, u32)]) -> TestResult {
    let after = kept_files(router_root)?;
    check(
        after == before,
        "the identity and enrollment files must keep their bytes and modes until PR B",
    )?;
    check(
        before.iter().all(|(_, _, mode)| *mode == 0o600),
        "enroll writes the identity and enrollment files owner-only",
    )?;
    let collaboration = router_root.join("agent-communication");
    check(
        collaboration_runtime::read_service_identity(&collaboration)?.is_some(),
        "the service identity reader accepts the kept file",
    )?;
    check(
        collaboration_runtime::read_installation_peer(&collaboration)?.is_some(),
        "the installation peer reader accepts the kept file",
    )?;
    let enrollment = codex_router_host::read_fleet_enrollment_document(
        &router_root.join("fleet/enrollment.json"),
        router_root,
    )?;
    let store = codex_router_host::open_fleet_enrollment_store(router_root)?;
    let (access_key, credential_key) = enrollment.secret_keys()?;
    for key in [&access_key, &credential_key] {
        codex_router_secret_store::SecretStore::read_secret(&store, key)?;
    }
    Ok(())
}

#[tokio::test]
async fn a_real_host_starts_ready_on_a_genuinely_enrolled_root_after_the_whole_update() -> TestResult
{
    let fixture = ProducerFixture::start().await?;
    let directory = fixture.root.path().to_owned();
    fixture
        .run(async |fixture| {
            let journey = whole_update_journey(fixture, &directory).await;
            // An app-server stand-in still running once the Host is gone is this
            // test's: signal it on every path, while the fixture directory exists. On a
            // passing journey the Host stopped it, so none may be left.
            let leftover = kill_fixture_processes(&directory);
            journey?;
            check(!leftover?, "no app-server stand-in may outlive the Host")
        })
        .await
}

/// Enrolls, adds the older layout, updates, and runs a real Host on the same root.
async fn whole_update_journey(fixture: &ProducerFixture, directory: &Path) -> TestResult {
    // Arrange: a real enrollment, then the older layout beside it.
    let router_root = &fixture.router_root;
    let expiry = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
    let enrolled = fleet_cli::run(fleet_cli::enrollment(
        router_root,
        &fixture.central,
        &expiry,
    )?)
    .await?;
    fleet_cli::require_success(&enrolled)?;
    let kept = kept_files(router_root)?;
    let ports = [
        reserve_loopback_port()?,
        reserve_loopback_port()?,
        reserve_loopback_port()?,
    ];
    add_older_layout(router_root, ports).await?;
    let codex_home = directory.join("codex");
    let socket_path = directory.join("dedicated-socket/app-server.sock");
    let managed = codex_home.join("packages/standalone/current/codex");
    std::fs::create_dir_all(managed.parent().ok_or("managed parent")?)?;
    install_managed_fixture(&managed)?;
    let launchctl = directory.join("launchctl");
    install_launchctl_fixture(&launchctl)?;
    install_debug_profile(&codex_home, ports[0])?;
    let binary = directory.join("codex-router");
    std::fs::copy(env!("CARGO_BIN_EXE_codex-router"), &binary)?;
    sign_debug_binary(&binary).await?;

    // Act: the whole update, then a real Host on the same root with no port flags.
    agent_router_update::run_router_update(router_root).await?;
    assert_kept_for_pr_b(router_root, &kept)?;
    // On a Mac the copy must carry the team identity right before it runs; Linux CI
    // has no codesign, and the runner leaves builds linker-signed there.
    #[cfg(target_os = "macos")]
    {
        let identity = tokio::process::Command::new("codesign")
            .arg("-dv")
            .arg(&binary)
            .output()
            .await?;
        check(
            String::from_utf8_lossy(&identity.stderr).contains("TeamIdentifier=974QD84WVC"),
            "the Host binary must be signed by the team identity before it is launched",
        )?;
    }
    let host_stderr = directory.join("host-stderr.log");
    let mut host = tokio::process::Command::new(&binary)
        .args(["host", "--require-debug-isolation", "--router-root"])
        .arg(router_root)
        .env("CODEX_ROUTER_USE_HOME_DEFAULT", "1")
        .env("OTEL_SDK_DISABLED", "true")
        .env("HOME", directory)
        .env("CODEX_HOME", &codex_home)
        .env("CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET", &socket_path)
        .env("CODEX_ROUTER_COMPILED_CLI_NATIVE_SOCKET", &socket_path)
        .env("CODEX_ROUTER_DEBUG_LAUNCHCTL", &launchctl)
        .env(
            "CODEX_ROUTER_COMPILED_CLI_LAUNCHCTL_LOG",
            directory.join("launchctl.log"),
        )
        .env(
            "CODEX_ROUTER_COMPILED_CLI_TEST_BINARY",
            std::env::current_exe()?,
        )
        .env("CODEX_ROUTER_COMPILED_CLI_APP_CHILD", "1")
        .env(
            "CODEX_ROUTER_COMPILED_CLI_PROCESS_LOG",
            directory.join("app-generations.log"),
        )
        .env("CODEX_ROUTER_COMPILED_CLI_MANAGED", &managed)
        .env("CODEX_ROUTER_DEBUG_READINESS_TIMING", "1")
        .env_remove(codex_router_host::inherited_lock_environment())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = host.stderr.take().ok_or("Host stderr unavailable")?;
    let ready = async {
        let drained = host_synchronized(stderr, &host_stderr).await?;
        let status = host_answer(&binary, router_root, &codex_home, &socket_path, "status").await?;
        let stdout = String::from_utf8(status.stdout)?;
        check(status.status.success(), &stdout)?;
        check(stdout.contains("readiness: ready"), &stdout)?;
        for (name, port) in ["router_port", "mcp_port", "tailscale_https_port"]
            .into_iter()
            .zip(ports)
        {
            check(
                stdout.contains(&format!("{name}: {port} (settings.jsonc ports)")),
                &stdout,
            )?;
        }
        // The Host attached to the central its kept enrollment names, with its secrets:
        // a project created through it reaches central, and only then is a session counted.
        let client = collaboration_client::CollaborationClient::connect(
            &router_root.join("agent-communication"),
            "whole-update-test",
            "1",
        )
        .await?;
        create_and_read_project(&client).await?;
        check(
            fixture.central.session_connections() > 0,
            "the Host must connect to the enrolled central",
        )?;
        Ok::<_, Box<dyn std::error::Error>>(drained)
    }
    .await;
    let stopped = host_answer(&binary, router_root, &codex_home, &socket_path, "stop").await;
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
    check(stopped?.status.success(), "the Host must stop cleanly")?;
    check(exited?.success(), "the Host must exit cleanly")?;
    assert_kept_for_pr_b(router_root, &kept)
}
