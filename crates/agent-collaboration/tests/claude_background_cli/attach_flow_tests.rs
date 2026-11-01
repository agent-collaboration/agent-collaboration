//! Real CLI and process boundaries; only the vendor executable is substituted.
use super::*;
use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, NativeClaudeJobReader};
use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
use collaboration_service::ApplicationFixture;
use serde_json::json;

const SESSION: &str = "00000000-0000-4000-8000-000000000042";
const RECEIPT: &str = include_str!("claude_bg_2_1_312.txt");

#[path = "resume_read_safety_tests.rs"]
mod resume_read_safety_tests;

fn background_new_command(
    fixture: &ClaudeProcessFixture,
    receipt: &str,
) -> std::io::Result<Command> {
    let mut command = resume_read_safety_tests::picker_command(fixture, SESSION)?;
    let arguments =
        ["--model", "haiku", "--name", "literal --bg value"].map(std::ffi::OsString::from);
    let request = json!({
        "serviceDirectory":fixture.root.path().join("router/agent-communication"),
        "expectedService":"00000000-0000-4000-8000-000000000001",
        "operation":{"kind":"nativeClaude","serviceEpoch":"00000000-0000-4000-8000-000000000002",
            "action":{"kind":"new","directory":fixture.working_directory,"arguments":arguments}}
    });
    command
        .env("CLAUDE_PICKER_REQUEST", request.to_string())
        .env("CLAUDE_PICKER_BACKGROUND_NEW", "1")
        .env("CLAUDE_FIXTURE_RECEIPT", receipt);
    Ok(command)
}

#[tokio::test]
async fn local_background_new_starts_once_then_attaches_supported_receipts() {
    for receipt in [RECEIPT, include_str!("claude_bg_unnamed.txt")] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, "working").unwrap();
        let output = background_new_command(&fixture, receipt)
            .unwrap()
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = invocations(&fixture).unwrap();
        assert_eq!(
            calls.len(),
            2,
            "one background start followed by one attach"
        );
        assert_eq!(
            calls[0].arguments,
            ["--bg", "--model", "haiku", "--name", "literal --bg value"]
        );
        assert_eq!(calls[1].arguments, ["attach", "a1b2c3d4"]);
        for call in calls {
            assert_eq!(call.working_directory, fixture.working_directory);
            assert_eq!(
                call.base_url,
                format!("http://{}/anthropic", fixture.endpoint)
            );
            assert_eq!(call.auth_token, "fixture-router-token");
        }
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn local_background_new_does_not_retry_or_attach_an_unparseable_receipt() {
    for receipt in [
        "no job handle\n",
        "backgrounded · a1b2c3d4\nbackgrounded · deadbeef\n",
    ] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, "working").unwrap();
        let output = background_new_command(&fixture, receipt)
            .unwrap()
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        let calls = invocations(&fixture).unwrap();
        assert_eq!(
            calls.len(),
            1,
            "an uncertain start must not attach, retry, or copy"
        );
        assert_eq!(calls[0].arguments[0], "--bg");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("claude agents")
                && diagnostic.contains("do not start another copy"),
            "{diagnostic}"
        );
        api.stop().await.unwrap();
    }
}

fn attach_command(fixture: &ClaudeProcessFixture) -> Command {
    let mut command = fixture.command();
    command
        .env(
            "CLAUDE_FIXTURE_FLOW",
            fixture.root.path().join("flow.jsonl"),
        )
        .env("CLAUDE_FIXTURE_RECEIPT", RECEIPT);
    command
}

fn invocations(fixture: &ClaudeProcessFixture) -> Result<Vec<ChildInvocation>, serde_json::Error> {
    fs::read_to_string(fixture.root.path().join("flow.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(serde_json::from_str)
        .collect()
}

fn serve_job(
    fixture: &ClaudeProcessFixture,
    state: &str,
) -> Result<ServedCollaborationApi, Box<dyn std::error::Error>> {
    let root = fixture.root.path();
    let peers = root.join("peers");
    fs::create_dir(&peers)?;
    let jobs = root.join("jobs.json");
    fs::write(
        &jobs,
        serde_json::to_vec(&json!([{
            "kind":"background", "id":"deadbeef", "sessionId":SESSION,
            "cwd":fixture.working_directory, "state":state, "startedAt":1790162100000_i64
        }]))?,
    )?;
    let reader = root.join("native-read");
    node_test_fixture::write_node_fixture(
        &reader,
        "native-cli-catalogue-agent.ts",
        &json!({"kind":"file","output":{"kind":"path","path":jobs}}),
    )?;
    fs::set_permissions(&reader, fs::Permissions::from_mode(0o700))?;
    let endpoint = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
        "label":"Native Claude", "channels":[],
        "availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"}
    }))?;
    let application = ApplicationFixture::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_endpoints(vec![endpoint])?
    .with_claude_code_sessions(Arc::new(ClaudeCodeSessionRegistry::new(peers)))
    .with_claude_background_jobs(Arc::new(NativeClaudeJobReader::for_executable(reader)))
    .build();
    let directory = root.join("router/agent-communication");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    Ok(ServedCollaborationApi::start_with(
        &directory,
        application,
        TestServeOptions {
            router_proxy_endpoint: Some(fixture.endpoint),
            ..TestServeOptions::default()
        },
    )?)
}

#[tokio::test]
async fn local_claude_new_foreground_opens_without_background_or_attach() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let output = attach_command(&fixture)
        .args([
            "--provider",
            "claude",
            "--new",
            "--",
            "--model",
            "claude-haiku-5-5",
            "--effort",
            "low",
        ])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(
        calls.len(),
        1,
        "foreground New must start one interactive client"
    );
    assert_eq!(
        calls[0].arguments,
        ["--model", "claude-haiku-5-5", "--effort", "low"]
    );
    assert_eq!(output.stdout, b"foreground fixture session\n");
    for call in calls {
        assert_eq!(call.working_directory, fixture.working_directory);
        assert_eq!(
            call.base_url,
            format!("http://{}/anthropic", fixture.endpoint)
        );
        assert_eq!(call.auth_token, "fixture-router-token");
    }
}

#[tokio::test]
async fn local_claude_running_background_attaches_without_starting_a_copy() {
    for state in ["working", "blocked"] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, state).unwrap();
        let output = attach_command(&fixture)
            .args(["--provider", "claude", "--id", SESSION])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{state}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = invocations(&fixture).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments,
            ["attach", "deadbeef"],
            "no --bg or --resume for {state}"
        );
        assert_eq!(calls[0].working_directory, fixture.working_directory);
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn local_claude_stopped_session_resumes_then_attaches_the_printed_handle() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    let output = attach_command(&fixture)
        .args(["--provider", "claude", "--id", SESSION])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(calls.len(), 2, "stopped Open must reach attach");
    assert_eq!(calls[0].arguments, ["--bg", "--resume", SESSION]);
    assert_eq!(calls[1].arguments, ["attach", "a1b2c3d4"]);
    api.stop().await.unwrap();
}

#[tokio::test]
async fn local_claude_resume_overrides_are_rejected_before_any_child_launch() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let output = attach_command(&fixture)
        .args([
            "--provider",
            "claude",
            "--id",
            SESSION,
            "--",
            "--model",
            "claude-haiku-5-5",
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(
        invocations(&fixture).unwrap().is_empty(),
        "override flags must not create a copy"
    );
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("without them to continue this session") && message.contains("Fork"),
        "{message}"
    );
}

#[tokio::test]
async fn local_claude_background_resume_rejects_a_now_running_job_before_starting_a_copy() {
    for state in ["working", "blocked"] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, state).unwrap();
        let request = json!({
            "serviceDirectory":fixture.root.path().join("router/agent-communication"),
            "expectedService":"00000000-0000-4000-8000-000000000001",
            "operation":{"kind":"nativeClaude","serviceEpoch":"00000000-0000-4000-8000-000000000002",
                "action":{"kind":"resume","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":SESSION},
                    "directory":fixture.working_directory,"arguments":[]}}
        });
        let output = attach_command(&fixture)
            .args(["--source-request-v1", &request.to_string()])
            .output()
            .await
            .unwrap();
        assert!(
            !output.status.success(),
            "a stale background Resume must not copy a now-{state} job"
        );
        assert!(
            invocations(&fixture).unwrap().is_empty(),
            "no bg --resume child for a running job"
        );
        assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 0);
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn local_claude_live_peer_with_a_historical_stopped_job_is_not_copied() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    let peers = fixture.root.path().join("peers");
    fs::write(
        peers.join(format!("{}.json", std::process::id())),
        serde_json::to_vec(&json!({
            "pid":std::process::id(), "sessionId":SESSION, "cwd":fixture.working_directory,
            "status":"idle", "startedAt":1790162100000_i64, "updatedAt":1790162101000_i64,
            "statusUpdatedAt":1790162101000_i64, "kind":"interactive", "entrypoint":"cli",
            "peerProtocol":1, "messagingSocketPath":peers.join("peer.sock")
        }))
        .unwrap(),
    )
    .unwrap();
    let output = attach_command(&fixture)
        .args(["--provider", "claude", "--id", SESSION])
        .output()
        .await
        .unwrap();
    assert!(
        !output.status.success(),
        "a historical stopped job does not make its live peer safe to resume again"
    );
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn local_claude_terminal_handoff_does_not_retarget_a_captured_directory() {
    use agent_session_inventory::sessions::{
        claude_launch_target::ClaudeLaunchTarget,
        claude_terminal_launch::{
            ClaudeTerminalAction, ClaudeTerminalRequest, prepare_claude_terminal,
        },
    };
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "working").unwrap();
    let moved_directory = fixture.root.path().join("moved project");
    fs::create_dir(&moved_directory).unwrap();
    fs::write(
        fixture.root.path().join("jobs.json"),
        serde_json::to_vec(&json!([{
            "kind":"background", "id":"deadbeef", "sessionId":SESSION,
            "cwd":moved_directory, "state":"working", "startedAt":1790162100000_i64
        }]))
        .unwrap(),
    )
    .unwrap();
    let launch = ClaudeLaunchTarget::resolve_for_service(
        None,
        fixture.root.path().join("router/agent-communication"),
    )
    .unwrap();
    let environment = launch.routed_environment().await.unwrap();
    let result = prepare_claude_terminal(
        &launch,
        ClaudeTerminalRequest {
            action: ClaudeTerminalAction::Resume {
                session_id: SESSION,
            },
            working_directory: Some(&fixture.working_directory),
        },
        environment,
    )
    .await;
    let error = result.expect_err("a stale captured directory must be rejected, not replaced");
    assert!(error.to_string().contains("directory changed"));
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}
