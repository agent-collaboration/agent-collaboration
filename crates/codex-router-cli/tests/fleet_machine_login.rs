//! Actual CLI → isolated production Host → C8 HTTP stand-in; no production socket or Keychain.
//! Central machine admission is deliberately outside this test and remains F14/SO8 proof.
use std::{
    collections::HashMap,
    error::Error,
    io,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Form, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
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
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

#[path = "support/fleet_host.rs"]
mod fleet_host;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000101";
const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000102";
const PEER: &str = "019f0000-0000-7000-8000-000000000103";
const STORE_ID: &str = "019f0000-0000-7000-8000-000000000104";
const ACCESS: &str =
    "eyJhbGciOiJSUzI1NiJ9.eyJleHAiOjYwMCwiZ3JhbnRfZXhwaXJlc19hdCI6NjAwfQ.access-secret-canary";
const DEVICE: &str = "f11-device-secret-canary";

struct IssuerState {
    authorizations: AtomicUsize,
    refreshes: AtomicUsize,
    client: Mutex<Option<String>>,
    refresh: Mutex<Option<String>>,
    refresh_observed: mpsc::UnboundedSender<HashMap<String, String>>,
}
struct Issuer {
    address: std::net::SocketAddr,
    state: Arc<IssuerState>,
    observed_refreshes: mpsc::UnboundedReceiver<HashMap<String, String>>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<io::Result<()>>,
}
impl Issuer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (refresh_observed, observed_refreshes) = mpsc::unbounded_channel();
        let state = Arc::new(IssuerState {
            authorizations: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
            client: Mutex::new(None),
            refresh: Mutex::new(None),
            refresh_observed,
        });
        let router = Router::new()
            .route("/device", post(authorize))
            .route("/token", post(token))
            .with_state(state.clone());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Self {
            address,
            state,
            observed_refreshes,
            stop,
            task,
        }
    }
    async fn shutdown(self) {
        self.stop.cancel();
        self.task.await.unwrap().unwrap();
    }
}
async fn authorize(
    State(state): State<Arc<IssuerState>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if form.get("organization_id").map(String::as_str) != Some(ORGANIZATION)
        || form.get("machine_installation_id").map(String::as_str) != Some(INSTALLATION)
        || form.get("machine_peer_id").map(String::as_str) != Some(PEER)
        || form.get("machine_label").map(String::as_str) != Some("CLI fixture machine")
        || form.get("client_id").map(String::as_str) != Some("fixture-public-device-client")
        || form.get("scope").map(String::as_str) != Some("offline_access")
        || form.get("resource").map(String::as_str)
            != Some("https://central.example.invalid/machine")
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response();
    }
    state.authorizations.fetch_add(1, Ordering::SeqCst);
    Json(json!({"device_code":DEVICE,"user_code":"ABCD-EFGH","verification_uri":"https://sign-in.example.invalid/activate","verification_uri_complete":"https://sign-in.example.invalid/activate?user_code=full-url-secret-canary","expires_in":600,"interval":5})).into_response()
}
async fn token(
    State(state): State<Arc<IssuerState>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    match form.get("grant_type").map(String::as_str) {
        Some("urn:ietf:params:oauth:grant-type:device_code")
            if form.get("device_code").map(String::as_str) == Some(DEVICE)
                && form.get("client_id").map(String::as_str)
                    == Some("fixture-public-device-client") =>
        {
            let ordinal = 0x200 + state.authorizations.load(Ordering::SeqCst);
            let client = format!("019f0000-0000-7000-8000-{ordinal:012x}");
            let refresh = format!("f11-refresh-secret-canary-{ordinal}");
            *state.client.lock().await = Some(client.clone());
            *state.refresh.lock().await = Some(refresh.clone());
            Json(json!({"access_token":ACCESS,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"machine_client_id":client})).into_response()
        }
        Some("refresh_token") => {
            let client = state.client.lock().await.clone();
            let mut current = state.refresh.lock().await;
            if form.get("client_id") != client.as_ref()
                || form.get("refresh_token") != current.as_ref()
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"invalid_grant"})),
                )
                    .into_response();
            }
            let ordinal = state.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            let refresh = format!("f11-rotated-refresh-secret-canary-{ordinal}");
            *current = Some(refresh.clone());
            let _sent = state.refresh_observed.send(form);
            Json(json!({"access_token":ACCESS,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"machine_client_id":client})).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response(),
    }
}

fn private_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn prepare_root(root: &Path, issuer: &Issuer, encrypted: bool) -> Option<EncryptedCredentialStore> {
    std::fs::write(
        root.join(fleet_host::MACHINE_LOGIN_FIXTURE_MARKER),
        b"injected-clock-and-keychain",
    )
    .unwrap();
    let router = root.join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    private_json(
        &directory.join("service-identity.json"),
        &json!({"version":1,"serviceId":INSTALLATION}),
    );
    private_json(
        &directory.join("installation-peer.json"),
        &json!({"version":1,"peerId":PEER}),
    );
    let fleet = router.join("fleet");
    std::fs::create_dir(&fleet).unwrap();
    std::fs::set_permissions(&fleet, std::fs::Permissions::from_mode(0o700)).unwrap();
    private_json(
        &fleet.join("login-config.json"),
        &json!({
            "deviceAuthorizationUrl":format!("http://{}/device", issuer.address),"tokenUrl":format!("http://{}/token", issuer.address),
            "deviceClientId":"fixture-public-device-client","machineResource":"https://central.example.invalid/machine",
            "endpoint":format!("ws://{}/session", issuer.address),"receiver":"019f0000-0000-7000-8000-000000000105", "relay":"019f0000-0000-7000-8000-000000000106","access":{"kind":"none"},
        }),
    );
    if !encrypted {
        return None;
    }
    let secrets = router.join("secrets");
    let files = FileSecretStore::open(&secrets).unwrap();
    std::fs::write(secrets.join("store-id"), STORE_ID).unwrap();
    std::fs::write(secrets.join("format-v2.marker"), b"2\n").unwrap();
    let store =
        EncryptedCredentialStore::new(files, PooledCredentialDataKey::from_bytes([0x54; 32]));
    let old = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        INSTALLATION,
        "019f0000-0000-7000-8000-000000000200",
    )
    .unwrap()
    .secret_key()
    .unwrap();
    store
        .write_staged(
            &old,
            &codex_router_core::redaction::SecretString::new("old-enrollment-secret-canary"),
        )
        .unwrap();
    Some(store)
}

async fn login(router: &Path) -> TestResult<std::process::Output> {
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
    codex_router_test_support::owned_cli_process::run_cli_output(command, Duration::from_secs(60))
        .await
        .map_err(io::Error::other)
        .map_err(Into::into)
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
        _ => Err("real board API did not match the login gate".into()),
    }
}

fn assert_no_secrets(root: &Path) {
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            assert_no_secrets(&entry.path());
        } else if kind.is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            for secret in [
                ACCESS,
                DEVICE,
                "f11-refresh-secret-canary",
                "full-url-secret-canary",
                "old-enrollment-secret-canary",
            ] {
                assert!(
                    !text.contains(secret),
                    "plaintext in {:?}",
                    entry.file_name()
                );
            }
        }
    }
}

#[tokio::test]
async fn fleet_machine_login_actual_cli_host_encryption_and_saved_identity_survive_restart() {
    let root = fleet_host::fixture_root().unwrap();
    let mut issuer = Issuer::start().await;
    let store = prepare_root(root.path(), &issuer, true).unwrap();
    let router = root.path().join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    let identity_before = std::fs::read(directory.join("service-identity.json")).unwrap();
    let peer_before = std::fs::read(directory.join("installation-peer.json")).unwrap();
    let result = fleet_host::run_host_scenario(root.path(), async |host| {
        host.await_ready().await?;
        let client = CollaborationClient::connect(&directory, "login-fixture", "1").await?;
        require_gate(&client, false).await?;
        let output = login(&router).await?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout).contains("ABCD-EFGH")
        {
            return Err("actual CLI login failed".into());
        }
        if !String::from_utf8_lossy(&output.stdout).contains("encrypted store") {
            return Err("CLI did not report encrypted keeping".into());
        }
        require_gate(&client, true).await?;
        Ok(())
    })
    .await;
    if let Err(error) = result {
        issuer.shutdown().await;
        panic!("first Host scenario failed: {error}");
    }
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(router.join("fleet/machine-login.json")).unwrap())
            .unwrap();
    let enrollment = metadata["enrollmentId"].as_str().unwrap().to_owned();
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
    assert_no_secrets(root.path());
    issuer.shutdown().await;
    result.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_actual_cli_memory_only_restart_requires_a_fresh_device_code() {
    let root = fleet_host::fixture_root().unwrap();
    let issuer = Issuer::start().await;
    assert!(prepare_root(root.path(), &issuer, false).is_none());
    let router = root.path().join("router");
    let directory = codex_router_host::router_collaboration_directory(&router);
    for expected_attempt in 1..=2 {
        let result = fleet_host::run_host_scenario(root.path(), async |host| {
            host.await_ready().await?;
            let client =
                CollaborationClient::connect(&directory, "memory-login-fixture", "1").await?;
            require_gate(&client, false).await?;
            let output = login(&router).await?;
            if !output.status.success()
                || !String::from_utf8_lossy(&output.stdout)
                    .contains("approve this machine again after a restart")
            {
                return Err("memory-only CLI login failed".into());
            }
            require_gate(&client, true).await?;
            Ok(())
        })
        .await;
        if let Err(error) = result {
            issuer.shutdown().await;
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
    assert_no_secrets(root.path());
    issuer.shutdown().await;
}
