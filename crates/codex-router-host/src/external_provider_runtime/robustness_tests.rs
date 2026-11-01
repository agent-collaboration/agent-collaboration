use super::*;
use collaboration_protocol::ProviderPromptStopReason;
use std::path::PathBuf;

#[cfg(unix)]
fn replay_fixture(update_count: usize) -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-pressure-agent.ts",
        vec!["replay".to_owned(), update_count.to_string()],
    )
}

#[cfg(unix)]
fn many_prompt_updates_fixture(update_count: usize) -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-pressure-agent.ts",
        vec!["updates".to_owned(), update_count.to_string()],
    )
}

#[cfg(unix)]
fn output_limit_and_late_update_fixture(control_path: &std::path::Path) -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-cancel-agent.ts",
        vec![control_path.to_string_lossy().into_owned()],
    )
}

#[cfg(unix)]
fn unknown_request_fixture() -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-unknown-request-agent.ts",
        vec!["session".to_owned()],
    )
}

#[cfg(unix)]
fn sessionless_unknown_request_fixture() -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-unknown-request-agent.ts",
        vec!["sessionless".to_owned()],
    )
}

#[cfg(unix)]
fn large_provider_frames_fixture() -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-pressure-agent.ts",
        vec!["frames".to_owned()],
    )
}

#[cfg(unix)]
fn large_permission_frame_fixture() -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-pressure-agent.ts",
        vec!["permission".to_owned()],
    )
}

#[cfg(unix)]
fn oversized_active_frame_fixture() -> ExternalProviderLaunch {
    acp_scripted_fixture::launch_fixture_agent(
        "host-output-pressure-agent.ts",
        vec!["limit".to_owned()],
    )
}

#[cfg(unix)]
#[tokio::test]
async fn provider_prompt_observes_more_than_the_old_update_limit_and_settles() {
    let runtime = ExternalProviderRuntime::initialize(many_prompt_updates_fixture(4097))
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        runtime.prompt("fixture-session".to_owned(), "many updates".to_owned()),
    )
    .await
    .expect("prompt settles")
    .expect("prompt succeeds");
    assert_eq!(outcome.output, "x".repeat(4097) + "DONE");
    assert_eq!(outcome.stop_reason, ProviderPromptStopReason::EndTurn);

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn load_replay_drains_more_than_the_old_update_limit_without_leaking_history() {
    let runtime = ExternalProviderRuntime::initialize(replay_fixture(4097))
        .await
        .expect("fixture initializes");
    runtime
        .load_session(
            "fixture-session".to_owned(),
            PathBuf::from("/tmp"),
            acp_client_runtime::ProviderSessionMcp::ConnectionDefaults,
        )
        .await
        .expect("large replay loads");

    let outcome = runtime
        .prompt("fixture-session".to_owned(), "next".to_owned())
        .await
        .expect("following prompt settles");
    assert_eq!(outcome.output, "CURRENT_OUTPUT");

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn load_replay_accepts_the_old_exact_update_boundary_without_history_leak() {
    let runtime = ExternalProviderRuntime::initialize(replay_fixture(4096))
        .await
        .expect("fixture initializes");
    runtime
        .load_session(
            "fixture-session".to_owned(),
            PathBuf::from("/tmp"),
            acp_client_runtime::ProviderSessionMcp::ConnectionDefaults,
        )
        .await
        .expect("exact-boundary replay loads");

    let outcome = runtime
        .prompt("fixture-session".to_owned(), "next".to_owned())
        .await
        .expect("following prompt settles");
    assert_eq!(outcome.output, "CURRENT_OUTPUT");

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn output_limit_cancels_and_settles_before_late_updates_can_reach_next_prompt() {
    let root = tempfile::tempdir().expect("fixture root");
    let control_path = root.path().join("late-output.sock");
    let control_listener = tokio::net::UnixListener::bind(&control_path).expect("control socket");
    let runtime =
        ExternalProviderRuntime::initialize(output_limit_and_late_update_fixture(&control_path))
            .await
            .expect("fixture initializes");
    let (mut control, _) =
        tokio::time::timeout(std::time::Duration::from_secs(2), control_listener.accept())
            .await
            .expect("fixture connects to control socket")
            .expect("control connection accepted");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");

    let error = runtime
        .prompt("fixture-session".to_owned(), "overflow".to_owned())
        .await
        .expect_err("retained output limit cancels prompt");
    assert!(matches!(
        error,
        ExternalProviderRuntimeError::PromptOutputLimitExceeded
    ));
    let mut barrier_signal = [0u8; 1];
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::io::AsyncReadExt::read_exact(&mut control, &mut barrier_signal),
    )
    .await
    .expect("unknown-request response barrier completes")
    .expect("control signal received");
    assert_eq!(barrier_signal, *b"r");
    let next = runtime
        .prompt("fixture-session".to_owned(), "next".to_owned())
        .await
        .expect("next prompt settles");
    assert_eq!(next.output, "CURRENT_OUTPUT");

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unknown_session_requests_are_rejected_idle_and_during_prompt() {
    let runtime = ExternalProviderRuntime::initialize(unknown_request_fixture())
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.prompt("fixture-session".to_owned(), "answer".to_owned()),
    )
    .await
    .expect("unknown requests do not park session actor")
    .expect("prompt settles");
    let responses: serde_json::Value =
        serde_json::from_str(&outcome.output).expect("request responses");
    for response in responses.as_array().expect("response list") {
        assert_eq!(response["error"]["code"], -32601, "{response}");
    }

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unknown_sessionless_requests_receive_method_not_found() {
    let runtime = ExternalProviderRuntime::initialize(sessionless_unknown_request_fixture())
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");
    let outcome = runtime
        .prompt("fixture-session".to_owned(), "answer".to_owned())
        .await
        .expect("prompt settles");
    let response: serde_json::Value = serde_json::from_str(&outcome.output).expect("response");
    assert_eq!(response["error"]["code"], -32601, "{response}");
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn request_naming_unknown_session_receives_method_not_found() {
    // ACP v1 JSON-RPC requests require a response. Specification R3 requires
    // -32601 for a request Router does not implement, even for an unknown ID.
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("unknown-session-answered");
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "unknown-session-fixture", "version": "1"}}))
        .expect_request("create", "session/new", serde_json::json!({}))
        .respond("create", serde_json::json!({"sessionId": "fixture-session"}))
        .send(serde_json::json!({"jsonrpc": "2.0", "id": 91, "method": "session/unknown", "params": {"sessionId": "not-loaded"}}))
        .expect_message(serde_json::json!({"jsonrpc": "2.0", "id": 91, "error": {"code": -32601}}))
        .write_marker(&marker)
        .exit()
        .record_diagnostics(root.path().join("fixture-diagnostics.txt"))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session created");
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.retirement().cancelled(),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "fixture did not finish unknown-session exchange: {}",
            std::fs::read_to_string(root.path().join("fixture-diagnostics.txt"))
                .unwrap_or_default()
        )
    });
    assert!(marker.exists(), "fixture did not observe -32601 response");
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn implemented_permission_request_during_load_replay_is_answered() {
    // ACP v1 session-setup.mdx:134-161 permits updates during session/load;
    // Specification R3 keeps implemented requests live during that replay.
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}, "agentInfo": {"name": "load-permission-fixture", "version": "1"}}))
        .expect_request("load", "session/load", serde_json::json!({"sessionId": "fixture-session"}))
        .send(serde_json::json!({"jsonrpc": "2.0", "id": 91, "method": "session/request_permission", "params": {"sessionId": "fixture-session", "toolCall": {"toolCallId": "replay-tool", "title": "Replay permission", "kind": "execute"}, "options": [{"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}}))
        .expect_message(serde_json::json!({"jsonrpc": "2.0", "id": 91, "result": {"outcome": {"outcome": "cancelled"}}}))
        .respond("load", serde_json::json!({}))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.load_session(
            "fixture-session".to_owned(),
            PathBuf::from("/tmp"),
            acp_client_runtime::ProviderSessionMcp::ConnectionDefaults,
        ),
    )
    .await
    .expect("permission request does not block load")
    .expect("session load succeeds");
    let observation = runtime.permission_observation();
    assert_eq!(observation.request_count, 1);
    assert_eq!(
        observation.last_outcome,
        Some(ExternalProviderPermissionOutcome::Cancelled)
    );
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unknown_update_kind_does_not_discard_later_prompt_output() {
    // ACP v1 prompt-turn.mdx:128-174 defines session/update streaming and
    // prompt-turn.mdx:336-367 defines the agent's final prompt result.
    // R4 extends the closed v1 update union without losing subsequent output.
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "unknown-update-fixture", "version": "1"}}))
        .expect_request("create", "session/new", serde_json::json!({}))
        .respond("create", serde_json::json!({"sessionId": "fixture-session"}))
        .expect_request("prompt", "session/prompt", serde_json::json!({"sessionId": "fixture-session"}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "before "}}}}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "future_update", "payload": "never echo this"}}}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "after"}}}}))
        .respond("prompt", serde_json::json!({"stopReason": "end_turn"}))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");
    let outcome = runtime
        .prompt("fixture-session".to_owned(), "continue".to_owned())
        .await
        .expect("unknown update does not end turn");
    assert_eq!(outcome.output, "before after");
    assert_eq!(outcome.stop_reason, ProviderPromptStopReason::EndTurn);
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unknown_stop_reason_has_sanitized_terminal_projection() {
    // ACP v1 prompt-turn.mdx:369-390 lists the known stop reasons. R4 keeps
    // an unrecognized future value without inventing one of those reasons.
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "unknown-stop-fixture", "version": "1"}}))
        .expect_request("create", "session/new", serde_json::json!({}))
        .respond("create", serde_json::json!({"sessionId": "fixture-session"}))
        .expect_request("prompt", "session/prompt", serde_json::json!({"sessionId": "fixture-session"}))
        .respond("prompt", serde_json::json!({"stopReason": "future_reason"}))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");
    let outcome = runtime
        .prompt("fixture-session".to_owned(), "continue".to_owned())
        .await
        .expect("unknown stop reason has a typed terminal outcome");
    assert_eq!(
        outcome.stop_reason,
        ProviderPromptStopReason::Unknown("future_reason".into())
    );
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_known_update_and_future_tool_status_do_not_end_turn() {
    // ACP v1 prompt-turn.mdx:128-174 defines the known chunk shape; R4 treats
    // a malformed known message as an error for that message only.
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "malformed-update-fixture", "version": "1"}}))
        .expect_request("create", "session/new", serde_json::json!({}))
        .respond("create", serde_json::json!({"sessionId": "fixture-session"}))
        .expect_request("prompt", "session/prompt", serde_json::json!({"sessionId": "fixture-session"}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "agent_message_chunk"}}}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "tool_call", "toolCallId": "future-tool", "title": "Future tool", "status": "future_status"}}}))
        .send(serde_json::json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fixture-session", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "after errors"}}}}))
        .respond("prompt", serde_json::json!({"stopReason": "end_turn"}))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");
    let outcome = runtime
        .prompt("fixture-session".to_owned(), "continue".to_owned())
        .await
        .expect("message errors do not end turn");
    assert_eq!(outcome.output, "after errors");
    assert_eq!(outcome.stop_reason, ProviderPromptStopReason::EndTurn);
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn permission_request_naming_unknown_session_receives_method_not_found() {
    // Specification R3: unknown session IDs receive -32601 even when the
    // method is implemented for loaded or currently loading Sessions.
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("unknown-permission-answered");
    let fixture = acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize", "initialize", serde_json::json!({"protocolVersion": 1}))
        .respond("initialize", serde_json::json!({"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "unknown-permission-fixture", "version": "1"}}))
        .expect_request("create", "session/new", serde_json::json!({}))
        .respond("create", serde_json::json!({"sessionId": "fixture-session"}))
        .send(serde_json::json!({"jsonrpc": "2.0", "id": 92, "method": "session/request_permission", "params": {"sessionId": "not-loaded", "toolCall": {"toolCallId": "unknown-tool", "title": "Unknown session permission", "kind": "execute"}, "options": [{"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}}))
        .expect_message(serde_json::json!({"jsonrpc": "2.0", "id": 92, "error": {"code": -32601}}))
        .write_marker(&marker)
        .exit()
        .record_diagnostics(root.path().join("fixture-diagnostics.txt"))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture)
        .await
        .expect("fixture initializes");
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session created");
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.retirement().cancelled(),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "fixture did not finish unknown-permission exchange: {}",
            std::fs::read_to_string(root.path().join("fixture-diagnostics.txt"))
                .unwrap_or_default()
        )
    });
    assert!(marker.exists(), "fixture did not observe -32601 response");
    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn large_tool_update_and_result_do_not_retire_other_provider_sessions() {
    let runtime = ExternalProviderRuntime::initialize(large_provider_frames_fixture())
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("first session created");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("second session created");

    let large_prompt = runtime.prompt("large-session".to_owned(), "large frames".to_owned());
    let second_prompt = runtime.prompt("second-session".to_owned(), "still alive".to_owned());
    let (large_result, second_result) = tokio::join!(large_prompt, second_prompt);
    let large_result = large_result.expect("large provider frames accepted");
    assert!(large_result.output.is_empty());
    assert_eq!(
        second_result
            .expect("second session remains available")
            .output,
        "SECOND_SESSION_OK"
    );

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn large_permission_request_frame_is_accepted_and_answered() {
    let runtime = ExternalProviderRuntime::initialize(large_permission_frame_fixture())
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");

    runtime
        .prompt("fixture-session".to_owned(), "large permission".to_owned())
        .await
        .expect("large permission request is answered");

    runtime.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn frame_limit_breach_returns_a_safe_typed_failure() {
    let runtime = ExternalProviderRuntime::initialize(oversized_active_frame_fixture())
        .await
        .expect("fixture initializes");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session created");

    let error = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        runtime.prompt(
            "fixture-session".to_owned(),
            "trigger frame breach".to_owned(),
        ),
    )
    .await
    .expect("frame breach terminates prompt")
    .expect_err("oversized provider frame is rejected");
    assert!(
        matches!(error, ExternalProviderRuntimeError::FrameLimitExceeded),
        "{error:?}"
    );

    runtime.shutdown().await;
}
