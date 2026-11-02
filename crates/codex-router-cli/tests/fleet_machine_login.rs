//! Actual CLI → isolated production Host → C8 HTTPS stand-in; no production socket or Keychain.
//! Central machine admission is deliberately outside this test and remains F14/SO8 proof.
use std::{
    error::Error, io, os::unix::fs::PermissionsExt, path::Path, process::Stdio,
    sync::atomic::Ordering,
};

use codex_router_secret_store::{
    SecretStore,
    encrypted_credential_store::EncryptedCredentialStore,
    file_backend::FileSecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    keychain_data_key::PooledCredentialDataKey,
};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{BoardFailureKind, BoardNextAction, PageRequest, WorkspaceListRequest};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

#[path = "support/machine_login_host.rs"]
mod fleet_host;

#[path = "../../codex-router-host/tests/support/fleet_login_https.rs"]
mod fleet_login_https;

#[path = "support/machine_login_issuer.rs"]
mod machine_login_issuer;
use machine_login_issuer::{DiscoveryReply, Issuer};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000101";
const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000102";
const PEER: &str = "019f0000-0000-7000-8000-000000000103";
const STORE_ID: &str = "019f0000-0000-7000-8000-000000000104";
const ACCESS: &str =
    "eyJhbGciOiJSUzI1NiJ9.eyJleHAiOjYwMCwiZ3JhbnRfZXhwaXJlc19hdCI6NjAwfQ.access-secret-canary";
const DEVICE: &str = "f11-device-secret-canary";
const SECRET_CANARIES: [&str; 6] = [
    ACCESS,
    DEVICE,
    "f11-refresh-secret-canary",
    "f11-rotated-refresh-secret-canary",
    "full-url-secret-canary",
    "old-enrollment-secret-canary",
];

fn private_json(path: &Path, value: &Value) -> TestResult {
    std::fs::write(path, serde_json::to_vec(value)?)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}
fn prepare_root(root: &Path, encrypted: bool) -> TestResult<Option<EncryptedCredentialStore>> {
    let router = root.join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    std::fs::create_dir(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    private_json(
        &directory.join("service-identity.json"),
        &json!({"version":1,"serviceId":INSTALLATION}),
    )?;
    private_json(
        &directory.join("installation-peer.json"),
        &json!({"version":1,"peerId":PEER}),
    )?;
    if !encrypted {
        return Ok(None);
    }
    let secrets = router.join("secrets");
    let files = FileSecretStore::open(&secrets)?;
    std::fs::write(secrets.join("store-id"), STORE_ID)?;
    std::fs::write(secrets.join("format-v2.marker"), b"2\n")?;
    let store =
        EncryptedCredentialStore::new(files, PooledCredentialDataKey::from_bytes([0x54; 32]));
    let old = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        INSTALLATION,
        "019f0000-0000-7000-8000-000000000200",
    )?
    .secret_key()?;
    store.write_staged(
        &old,
        &codex_router_core::redaction::SecretString::new("old-enrollment-secret-canary"),
    )?;
    Ok(Some(store))
}

async fn login(router: &Path, sign_in: Option<&str>) -> TestResult<std::process::Output> {
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let mut command = tokio::process::Command::new(runner);
    command
        .arg(env!("CARGO_BIN_EXE_codex-router"))
        .args([
            "fleet",
            "login",
            "--organization",
            ORGANIZATION,
            "--label",
            "CLI fixture machine",
            "--router-root",
        ])
        .arg(router)
        .env("OTEL_SDK_DISABLED", "true");
    if let Some(address) = sign_in {
        command.arg("--sign-in").arg(address);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let Some((mut stdout, mut stderr)) = child.stdout.take().zip(child.stderr.take()) else {
        let _kill = child.start_kill();
        child.wait().await?;
        return Err(io::Error::other("CLI fixture did not publish output pipes").into());
    };
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    // Process exit and pipe EOF are the only completion events. F11 does not
    // add an elapsed-time guard around the existing production exchange.
    match tokio::try_join!(
        child.wait(),
        stdout.read_to_end(&mut stdout_bytes),
        stderr.read_to_end(&mut stderr_bytes)
    ) {
        Ok((status, _, _)) => {
            for bytes in [&stdout_bytes, &stderr_bytes] {
                let text = String::from_utf8_lossy(bytes);
                if SECRET_CANARIES.iter().any(|secret| text.contains(secret)) {
                    return Err("CLI output exposed a fixture secret".into());
                }
            }
            Ok(std::process::Output {
                status,
                stdout: stdout_bytes,
                stderr: stderr_bytes,
            })
        }
        Err(error) => {
            let _kill = child.start_kill();
            child.wait().await?;
            Err(error.into())
        }
    }
}

fn failed_cli(output: &std::process::Output, context: &str) -> Box<dyn Error> {
    // The executable speaks only public display events/errors here. Redact all
    // fixture canaries anyway so a failing privacy assertion never prints one.
    let redact = |bytes: &[u8]| {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        for canary in SECRET_CANARIES.into_iter().chain(["ABCD-EFGH"]) {
            text = text.replace(canary, "[REDACTED]");
        }
        text.chars().take(4_096).collect::<String>()
    };
    io::Error::other(format!(
        "{context}: status={}; stdout={:?}; stderr={:?}",
        output.status,
        redact(&output.stdout),
        redact(&output.stderr)
    ))
    .into()
}
async fn require_gate(client: &CollaborationClient, logged_in: bool) -> TestResult {
    let result = client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest::default(),
        })
        .await;
    match (logged_in, result) {
        (true, Ok(_)) => Ok(()),
        (false, Err(BoardClientError::Rejected(error)))
            if error.kind == BoardFailureKind::NotLoggedIn
                && error.next_action == BoardNextAction::RunFleetLogin =>
        {
            Ok(())
        }
        (expected, Err(BoardClientError::Rejected(error))) => Err(io::Error::other(format!(
            "board gate expected logged_in={expected}; observed {:?}/{:?}",
            error.kind, error.next_action
        ))
        .into()),
        (false, Ok(_)) => Err("board API admitted a machine before login".into()),
        _ => Err("real board API transport did not establish login admission".into()),
    }
}

async fn held_login(
    issuer: &mut Issuer,
    router: &Path,
    client: &CollaborationClient,
    sign_in: Option<&str>,
) -> TestResult<std::process::Output> {
    let command = login(router, sign_in);
    tokio::pin!(command);
    tokio::select! {
        event = issuer.observed_discoveries.recv() => event.ok_or("discovery did not begin")?,
        result = &mut command => { let output = result?; return Err(failed_cli(&output, "CLI stopped before held discovery")); },
    }
    // The real command has begun; the issuer has not returned metadata yet.
    require_gate(client, false).await?;
    issuer.state.release_discovery.add_permits(1);
    tokio::select! {
        event = issuer.observed_redemptions.recv() => event.ok_or("redemption did not begin")?,
        result = &mut command => { let output = result?; return Err(failed_cli(&output, "CLI stopped before held grant")); },
    }
    require_gate(client, false).await?;
    issuer.state.release_redemption.add_permits(1);
    command.await
}

async fn saved_login(
    router: &Path,
) -> TestResult<Option<codex_router_fleet_state::fleet_login::SavedFleetLogin>> {
    use codex_router_fleet_state::fleet_login_repository::FleetLoginRepository;
    let store = codex_router_fleet_state::FleetStateStore::open_read_only(router).await?;
    let saved = store.load_fleet_login().await;
    store.close().await;
    Ok(saved?)
}

fn assert_no_secrets(root: &Path) -> TestResult {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            assert_no_secrets(&entry.path())?;
        } else if kind.is_file() {
            let bytes = std::fs::read(entry.path())?;
            let text = String::from_utf8_lossy(&bytes);
            for secret in SECRET_CANARIES.into_iter().chain(["ABCD-EFGH"]) {
                if text.contains(secret) {
                    return Err(
                        io::Error::other(format!("plaintext in {:?}", entry.file_name())).into(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn assert_live_database_privacy(root: &Path) -> TestResult {
    for name in ["fleet.sqlite", "fleet.sqlite-wal", "fleet.sqlite-shm"] {
        let path = root.join("router").join(name);
        if !path.is_file() {
            return Err(format!("live SQLite privacy scan missing {name}").into());
        }
        if std::fs::metadata(path)?.permissions().mode() & 0o777 != 0o600 {
            return Err(format!("live SQLite file is not owner-private: {name}").into());
        }
    }
    assert_no_secrets(root)
}

#[tokio::test]
async fn fleet_machine_login_actual_cli_host_encryption_and_saved_identity_survive_restart() {
    let root = fleet_host::fixture_root().unwrap();
    let mut issuer = Issuer::start(DiscoveryReply::Complete).await.unwrap();
    let store = prepare_root(root.path(), true).unwrap().unwrap();
    let router = root.path().join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    let identity_before = std::fs::read(directory.join("service-identity.json")).unwrap();
    let peer_before = std::fs::read(directory.join("installation-peer.json")).unwrap();
    let result = fleet_host::run_host_scenario(root.path(), async |host| {
        host.await_ready().await?;
        let client = CollaborationClient::connect(&directory, "login-fixture", "1").await?;
        require_gate(&client, false).await?;
        let sign_in = issuer.sign_in();
        let output = held_login(&mut issuer, &router, &client, Some(&sign_in)).await?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout).contains("ABCD-EFGH")
        {
            return Err(failed_cli(&output, "actual CLI login failed"));
        }
        if !String::from_utf8_lossy(&output.stdout).contains("encrypted store") {
            return Err("CLI did not report encrypted keeping".into());
        }
        // The learned route reassembles the public runtime; reconnect normally.
        let reconnected =
            CollaborationClient::connect(&directory, "login-fixture-after-adoption", "1").await?;
        require_gate(&reconnected, true).await?;
        assert_live_database_privacy(root.path())?;
        Ok(())
    })
    .await;
    if let Err(error) = result {
        issuer.shutdown().await.unwrap();
        panic!("first Host scenario failed: {error}");
    }
    let saved = saved_login(&router).await.unwrap().unwrap();
    assert_eq!(saved.sign_in.as_str(), issuer.sign_in());
    assert_eq!(
        saved.central.receiver.to_string(),
        "019f0000-0000-7000-8000-000000000105"
    );
    assert!(!router.join("fleet/login-config.json").exists());
    assert!(!router.join("fleet/machine-login.json").exists());
    let enrollment = saved.enrollment_id.as_str().to_owned();
    let active = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        INSTALLATION,
        &enrollment,
    )
    .unwrap()
    .secret_key()
    .unwrap();
    let bundle: Value =
        serde_json::from_str(store.read_secret(&active).unwrap().expose_secret()).unwrap();
    assert_eq!(bundle["machineClientId"], enrollment);
    assert_eq!(bundle.as_object().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_dir(router.join("secrets"))
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "v2"))
            .count(),
        1
    );
    let result = fleet_host::run_host_scenario(root.path(), async |host| {
        host.await_ready().await?;
        let client =
            CollaborationClient::connect(&directory, "login-fixture-before-restoration", "1")
                .await?;
        require_gate(&client, false).await?;
        issuer
            .observed_discoveries
            .recv()
            .await
            .ok_or("restart discovery missing")?;
        issuer.state.release_discovery.add_permits(1);
        let refreshed = issuer
            .observed_refreshes
            .recv()
            .await
            .ok_or("automatic restart refresh missing")?;
        if refreshed.get("client_id").map(String::as_str) != Some(enrollment.as_str()) {
            return Err("restart used the shared device client".into());
        }
        Ok(())
    })
    .await;
    assert_eq!(
        std::fs::read(directory.join("service-identity.json")).unwrap(),
        identity_before
    );
    assert_eq!(
        std::fs::read(directory.join("installation-peer.json")).unwrap(),
        peer_before
    );
    assert_no_secrets(root.path()).unwrap();
    issuer.shutdown().await.unwrap();
    result.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_actual_cli_memory_only_restart_requires_a_fresh_device_code() {
    let root = fleet_host::fixture_root().unwrap();
    let mut issuer = Issuer::start(DiscoveryReply::Complete).await.unwrap();
    assert!(prepare_root(root.path(), false).unwrap().is_none());
    let router = root.path().join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    for expected_attempt in 1..=2 {
        let result = fleet_host::run_host_scenario(root.path(), async |host| {
            host.await_ready().await?;
            let client =
                CollaborationClient::connect(&directory, "memory-login-fixture", "1").await?;
            require_gate(&client, false).await?;
            let sign_in = issuer.sign_in();
            let output = held_login(
                &mut issuer,
                &router,
                &client,
                (expected_attempt == 1).then_some(sign_in.as_str()),
            )
            .await?;
            if !output.status.success()
                || !String::from_utf8_lossy(&output.stdout)
                    .contains("approve this machine again after a restart")
            {
                return Err(failed_cli(&output, "memory-only CLI login failed"));
            }
            let reconnected = CollaborationClient::connect(
                &directory,
                "memory-login-fixture-after-adoption",
                "1",
            )
            .await?;
            require_gate(&reconnected, true).await?;
            if saved_login(&router).await?.is_none() {
                return Err("memory login did not save its public route".into());
            }
            assert_live_database_privacy(root.path())?;
            Ok(())
        })
        .await;
        if let Err(error) = result {
            issuer.shutdown().await.unwrap();
            panic!("memory Host scenario failed: {error}");
        }
        assert_eq!(
            issuer.state.authorizations.load(Ordering::SeqCst),
            expected_attempt
        );
        assert_eq!(issuer.state.refreshes.load(Ordering::SeqCst), 0);
    }
    let machine_credentials = match std::fs::read_dir(router.join("secrets")) {
        Ok(entries) => entries
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("fleet_machine_credential.")
            })
            .count(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => panic!("cannot inspect memory-only secret root: {error}"),
    };
    assert_eq!(machine_credentials, 0);
    assert_no_secrets(root.path()).unwrap();
    issuer.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_actual_cli_rejects_discovery_before_saving_and_keeps_rowless_mode_on_invalid_input()
 {
    for discovery_reply in [
        DiscoveryReply::ExtraClientField,
        DiscoveryReply::IssuerMismatch,
    ] {
        let root = fleet_host::fixture_root().unwrap();
        let issuer = Issuer::start(discovery_reply).await.unwrap();
        assert!(prepare_root(root.path(), false).unwrap().is_none());
        let router = root.path().join("router");
        let directory = codex_router_host::router_collaboration_directory(&router);
        let result = fleet_host::run_host_scenario(root.path(), async |host| {
            host.await_ready().await?;
            let client =
                CollaborationClient::connect(&directory, "discovery-refusal-fixture", "1").await?;
            require_gate(&client, false).await?;
            let missing = login(&router, None).await?;
            if missing.status.success()
                || !String::from_utf8_lossy(&missing.stderr).contains("no sign-in address is saved")
            {
                return Err(failed_cli(&missing, "missing sign-in was not refused"));
            }
            require_gate(&client, false).await?;
            let invalid = login(&router, Some("http://issuer.example.invalid/api/auth")).await?;
            if invalid.status.success()
                || !String::from_utf8_lossy(&invalid.stderr).contains("HTTPS issuer")
            {
                return Err(failed_cli(&invalid, "HTTP issuer was not refused"));
            }
            require_gate(&client, false).await?;
            issuer.state.release_discovery.add_permits(1);
            let refused = login(&router, Some(&issuer.sign_in())).await?;
            if refused.status.success()
                || !String::from_utf8_lossy(&refused.stderr).contains("invalid login response")
            {
                return Err(failed_cli(&refused, "invalid discovery was not refused"));
            }
            require_gate(&client, false).await?;
            if saved_login(&router).await?.is_some() {
                return Err("invalid discovery published a fleet_login row".into());
            }
            if issuer.state.authorizations.load(Ordering::SeqCst) != 0 {
                return Err("invalid discovery reached device authorization".into());
            }
            Ok(())
        })
        .await;
        assert_no_secrets(root.path()).unwrap();
        issuer.shutdown().await.unwrap();
        result.unwrap();
    }
}
