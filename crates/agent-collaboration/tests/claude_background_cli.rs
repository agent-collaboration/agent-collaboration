//! Public-entry process wiring proof. The executable substitutes only native Claude;
//! its receipt is fixture output, never evidence of vendor background registration.
use serde::Deserialize;
use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    process::Command,
};

#[derive(Debug, Deserialize)]
struct ChildInvocation {
    arguments: Vec<String>,
    working_directory: PathBuf,
    base_url: String,
    auth_token: String,
}

struct ClaudeProcessFixture {
    root: tempfile::TempDir,
    working_directory: PathBuf,
    endpoint: std::net::SocketAddr,
    health_requests: Arc<AtomicUsize>,
    health_server: tokio::task::JoinHandle<()>,
}

impl ClaudeProcessFixture {
    async fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("claude-background-")
            .tempdir_in("/private/tmp")
            .expect("private process fixture");
        let working_directory = root.path().join("original project 東京");
        for directory in [
            "bin",
            "home",
            "router/agent-communication",
            "router/secrets",
        ] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        fs::create_dir(&working_directory).unwrap();
        fs::write(
            root.path().join("router/secrets/local_router_token.secret"),
            "fixture-router-token",
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        fs::write(
            root.path().join("router/agent-communication/service.json"),
            serde_json::json!({
                "version":3,"serviceId":"00000000-0000-4000-8000-000000000001",
                "machineLabel":"fixture","serviceEpoch":"00000000-0000-4000-8000-000000000002",
                "serviceVersion":env!("CARGO_PKG_VERSION"),
                "api":{"transport":"streamableHttpUnix","path":"control.sock"},
                "mcp":{"transport":"streamableHttp","url":"http://127.0.0.1:18788/mcp"},
                "routerProxyEndpoint":endpoint.to_string(),
            })
            .to_string(),
        )
        .unwrap();
        let health_requests = Arc::new(AtomicUsize::new(0));
        let observed_requests = health_requests.clone();
        let health_server = tokio::spawn(async move {
            loop {
                let (mut connection, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let byte = connection.read_u8().await.unwrap();
                    request.push(byte);
                    assert!(request.len() < 4096);
                }
                assert!(request.starts_with(b"GET /healthz HTTP/1.1\r\n"));
                observed_requests.fetch_add(1, Ordering::SeqCst);
                connection
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
                    )
                    .await
                    .unwrap();
            }
        });
        let executable = root.path().join("bin/claude");
        fs::write(&executable,r#"#!/usr/bin/python3
import json, os, sys
with open(os.environ['CLAUDE_FIXTURE_COUNT'], 'a') as count: count.write('launch\n')
with open(os.environ['CLAUDE_FIXTURE_RECORD'], 'w') as record:
    json.dump({'arguments':sys.argv[1:], 'working_directory':os.getcwd(), 'base_url':os.environ.get('ANTHROPIC_BASE_URL'), 'auth_token':os.environ.get('ANTHROPIC_AUTH_TOKEN')}, record)
if not os.environ.get('CLAUDE_FIXTURE_NO_RECEIPT'):
    print('bg-fixture-receipt-001', flush=True)
if os.environ.get('CLAUDE_FIXTURE_STDERR'):
    print(os.environ['CLAUDE_FIXTURE_STDERR'], file=sys.stderr, flush=True)
sys.exit(int(os.environ.get('CLAUDE_FIXTURE_EXIT', '0')))
"#).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            working_directory,
            endpoint,
            health_requests,
            health_server,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-sessions"));
        command
            .kill_on_drop(true)
            .current_dir(&self.working_directory)
            .env("HOME", self.root.path().join("home"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .env(
                "CODEX_ROUTER_DEBUG_ROUTER_ROOT",
                self.root.path().join("router"),
            )
            .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
            .env(
                "CLAUDE_FIXTURE_RECORD",
                self.root.path().join("invocation.json"),
            )
            .env(
                "CLAUDE_FIXTURE_COUNT",
                self.root.path().join("invocations.txt"),
            )
            .env("ANTHROPIC_BASE_URL", "parent-route-must-be-overridden")
            .env("ANTHROPIC_AUTH_TOKEN", "parent-token-must-be-overridden");
        command
    }

    fn invocation(&self) -> ChildInvocation {
        serde_json::from_slice(
            &fs::read(self.root.path().join("invocation.json"))
                .expect("native executable must have run"),
        )
        .unwrap()
    }
}
impl Drop for ClaudeProcessFixture {
    fn drop(&mut self) {
        self.health_server.abort();
    }
}

#[tokio::test]
async fn literal_claude_new_starts_native_background_and_preserves_routed_process_boundary() {
    let fixture = ClaudeProcessFixture::new().await;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .command()
            .args(["--provider", "claude", "--new"])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fixture.invocation();
    assert_eq!(
        invocation.arguments,
        ["--bg"],
        "literal public New must choose native background mode"
    );
    assert_eq!(
        invocation.working_directory,
        fs::canonicalize(&fixture.working_directory).unwrap()
    );
    assert_eq!(
        invocation.base_url,
        format!("http://{}/anthropic", fixture.endpoint)
    );
    assert_eq!(invocation.auth_token, "fixture-router-token");
    assert_eq!(
        output.stdout, b"bg-fixture-receipt-001\n",
        "preserve the child receipt, rather than invent a background identity"
    );
    assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn claude_resume_preserves_background_aliases_and_all_custom_arguments() {
    let fixture = ClaudeProcessFixture::new().await;
    let session_id = "018f47d2-24d5-7a68-b9ec-6f759c39458f";
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .command()
            .args([
                "--provider",
                "claude",
                "--id",
                session_id,
                "--",
                "--background",
                "--bg",
                "--model",
                "opus",
                "--effort",
                "low",
                "--fork-session",
                "Describe 東京 project without changing it",
            ])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fixture.invocation();
    assert_eq!(
        invocation.arguments,
        [
            "--bg",
            "--resume",
            session_id,
            "--background",
            "--bg",
            "--model",
            "opus",
            "--effort",
            "low",
            "--fork-session",
            "Describe 東京 project without changing it"
        ]
    );
    assert_eq!(
        invocation.working_directory,
        fs::canonicalize(&fixture.working_directory).unwrap()
    );
    assert_eq!(
        invocation.base_url,
        format!("http://{}/anthropic", fixture.endpoint)
    );
    assert_eq!(invocation.auth_token, "fixture-router-token");
    assert_eq!(output.stdout, b"bg-fixture-receipt-001\n");
}

// This substitutes only a native nonzero outcome. Actual Claude guard ordering
// and zero session/model effects require independent installed-vendor proof.
#[tokio::test]
async fn claude_print_arguments_reach_native_cli_and_its_failure_is_preserved() {
    for flag in ["-p", "--print", "--print=true", "-phello"] {
        let fixture = ClaudeProcessFixture::new().await;
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            fixture
                .command()
                .env("CLAUDE_FIXTURE_EXIT", "9")
                .env("CLAUDE_FIXTURE_NO_RECEIPT", "1")
                .env("CLAUDE_FIXTURE_STDERR", "fixture-native-print-conflict")
                .args(["--provider", "claude", "--new", "--", flag])
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            fixture.invocation().arguments,
            ["--bg", flag],
            "vendor option tokens must remain unchanged"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("fixture-native-print-conflict") && stderr.contains("exit status: 9"),
            "{stderr}"
        );
        assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 1);
        assert!(output.stdout.is_empty(), "no invented success receipt");
    }
}

#[tokio::test]
async fn claude_background_preserves_actual_nonzero_exit_and_reports_spawn_failure() {
    let fixture = ClaudeProcessFixture::new().await;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .command()
            .env("CLAUDE_FIXTURE_EXIT", "7")
            .args(["--provider", "claude", "--new"])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Claude Code exited with exit status: 7")
    );
    assert_eq!(output.stdout, b"bg-fixture-receipt-001\n");
    assert_eq!(fixture.invocation().arguments, ["--bg"]);

    let missing = ClaudeProcessFixture::new().await;
    fs::rename(
        missing.root.path().join("bin/claude"),
        missing.root.path().join("bin/unused-fixture"),
    )
    .unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        missing
            .command()
            .args(["--provider", "claude", "--new"])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to launch Claude Code"));
    assert!(!missing.root.path().join("invocation.json").exists());
    assert_eq!(missing.health_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn claude_background_preserves_required_option_values_that_resemble_modes() {
    let fixture = ClaudeProcessFixture::new().await;
    let arguments = [
        "--append-system-prompt",
        "-p literal policy",
        "--name",
        "--background",
        "--model",
        "opus",
        "--effort",
        "low",
    ];
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .command()
            .args(["--provider", "claude", "--new", "--"])
            .args(arguments)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "option values are data: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fixture.invocation();
    let expected = std::iter::once("--bg").chain(arguments).collect::<Vec<_>>();
    assert_eq!(
        invocation.arguments, expected,
        "all supplied values must reach the same native process boundary unchanged"
    );
}

#[path = "claude_background_cli/native_source_tests.rs"]
mod native_source_tests;
