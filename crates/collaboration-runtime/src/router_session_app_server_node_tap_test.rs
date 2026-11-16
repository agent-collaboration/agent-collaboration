//! N1 node taps: one catalog input per disposition, driven through the real
//! request dispatcher with a scripted session backend at the N4 edge. The
//! connection-loop cases live in `router_session_app_server_connection_node_tap_test.rs`.
use super::test_support::ScriptedSessionBackend;
use super::*;
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use wire::CliConnectionRecorder;

#[path = "router_session_app_server_node_tap_wire_test.rs"]
pub(super) mod wire;

const N1: ProtocolNode = ProtocolNode::N1CliFace;
const N4: EdgeParty = EdgeParty::Node(ProtocolNode::N4CommandPort);

pub(super) fn new_context(
    backend: &Arc<ScriptedSessionBackend>,
) -> Arc<RouterSessionAppServerContext> {
    Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor().expect("actor"),
        Arc::clone(backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(backend) as Arc<dyn SessionEventHub>,
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ))
}

/// Generated per request (a fresh UUIDv7), so the recording replaces only this field with a fixed
/// sentinel; every other field of each command is recorded and compared.
const VOLATILE_INPUT_ID: &str = "<volatile:input-id>";

fn volatile_input_id() -> session_event_model::InputId {
    session_event_model::InputId::new(VOLATILE_INPUT_ID).expect("sentinel input id")
}

/// The whole command as its `Debug` text (the command types have no serde).
fn whole_command(command: &impl std::fmt::Debug) -> Value {
    json!({"debug": format!("{command:#?}")})
}

/// What N1 actually sent toward N4: each whole recorded session command, with its kind.
pub(super) fn commands_seen_at_n4(backend: &ScriptedSessionBackend) -> Vec<(String, Value)> {
    let mut seen = Vec::new();
    for command in backend.create_commands.lock().expect("lock").iter() {
        seen.push(("Create".to_owned(), whole_command(command)));
    }
    for command in backend.prompt_commands.lock().expect("lock").iter() {
        let mut command = command.clone();
        command.input_id = volatile_input_id();
        seen.push(("Prompt".to_owned(), whole_command(&command)));
    }
    for command in backend.steer_commands.lock().expect("lock").iter() {
        let mut command = command.clone();
        command.input_id = volatile_input_id();
        seen.push(("Steer".to_owned(), whole_command(&command)));
    }
    for command in backend.cancel_commands.lock().expect("lock").iter() {
        seen.push(("Interrupt".to_owned(), whole_command(command)));
    }
    for command in backend.compact_commands.lock().expect("lock").iter() {
        seen.push(("Compact".to_owned(), whole_command(command)));
    }
    for command in backend.setting_commands.lock().expect("lock").iter() {
        seen.push((
            format!("SetSetting({})", command.setting_id),
            whole_command(command),
        ));
    }
    seen
}

/// Literal session and actor, written independently of the scripted backend's own fixtures.
fn literal_session() -> message_board::SessionRef {
    serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"19e49a31-daa3-428c-b985-e0c7373a89ed"
    }))
    .expect("session")
}

fn literal_actor() -> message_board::Identity {
    serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor")
}

fn literal_text_submission(text: &str) -> session_event_model::ProviderSubmission {
    session_event_model::ProviderSubmission {
        content: vec![session_event_model::PromptContent::Text {
            text: session_event_model::PromptText::try_new(text.to_owned()).expect("text"),
        }],
        selected: Vec::new(),
    }
}

pub(super) fn state_with_commands(count: usize) -> NodeStateSnapshot {
    NodeStateSnapshot::new().field("session_commands_issued", count)
}

/// Drives one CLI request through the dispatcher and records, at the CLI edge, the request value
/// the dispatcher took (`TypedInput`), and the response and post-response frames it returned
/// (`TypedOutcome`): a dispatcher case never runs the connection loop, so none of these is a wire
/// frame; at the N4 edge, every session command issued after the first `issued_before`.
async fn dispatch_recorded(
    case: &mut TapCase,
    backend: &Arc<ScriptedSessionBackend>,
    request: Value,
    issued_before: usize,
) -> HandledAppServerRequest {
    let mut connection = CliConnectionRecorder::dispatcher_values();
    connection.cli_sent(case, &request);
    let handled = request_dispatch::handle_app_server_rpc(new_context(backend), request)
        .await
        .expect("dispatch");
    for (kind, command) in commands_seen_at_n4(backend).into_iter().skip(issued_before) {
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionCommand,
            N4,
            &kind,
            command,
        );
    }
    connection.n1_sent(case, &handled.response);
    for frame in &handled.post_response_frames {
        connection.n1_sent(case, frame);
    }
    handled
}

/// Drives one request through the dispatcher and records every edge message.
async fn tap_dispatch(
    case: &mut TapCase,
    backend: &Arc<ScriptedSessionBackend>,
    request: Value,
) -> HandledAppServerRequest {
    case.state_before(state_with_commands(commands_seen_at_n4(backend).len()));
    let handled = dispatch_recorded(case, backend, request, 0).await;
    case.state_after(state_with_commands(commands_seen_at_n4(backend).len()));
    handled
}

async fn tap_refusal(case_name: &str, row: &str, method: &str, params: Value, message: &str) {
    // Arrange
    let backend = ScriptedSessionBackend::new().expect("backend");
    let mut case = TapCase::new(N1, case_name, row);
    // Act
    let handled = tap_dispatch(
        &mut case,
        &backend,
        json!({"id":5,"method":method,"params":params}),
    )
    .await;
    // Assert: a visible -32000 error with a reason, never a result, no session command.
    assert!(
        handled.response.get("result").is_none()
            && handled.response["error"]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
        "{}",
        handled.response
    );
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            &format!("error:-32000:{method}"),
            json!({"id":5,"error":{"code":-32000,"message":message}}),
        )
        .state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_refused_host_shell() {
    tap_refusal(
        "refused_host_shell",
        "Dispositions: Refused HostShell",
        "command/exec",
        json!({"command":["ls"]}),
        "command/exec is not available through the Router: this endpoint does not provide host shell commands or terminals",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_context_clone_or_inject() {
    tap_refusal(
        "refused_context_clone_or_inject",
        "Dispositions: Refused ContextCloneOrInject",
        "thread/fork",
        json!({"threadId":"t"}),
        "thread/fork is not available through the Router: provider conversation context cannot be cloned or injected here",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_session_metadata_read_only() {
    tap_refusal(
        "refused_session_metadata_read_only",
        "Dispositions: Refused SessionMetadataReadOnly",
        "thread/name/set",
        json!({"threadId":"t","name":"x"}),
        "thread/name/set is not available through the Router: provider-session metadata cannot be changed here",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_native_queue() {
    tap_refusal(
        "refused_native_queue",
        "Dispositions: Refused NativeQueue",
        "thread/queue/add",
        json!({"threadId":"t","clientUserMessageId":"client-1","input":[{"type":"text","text":"hello"}]}),
        "thread/queue/add is not available through the Router: the native thread queue is not exposed on this endpoint",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_codex_review() {
    tap_refusal(
        "refused_codex_review",
        "Dispositions: Refused CodexReview",
        "review/start",
        json!({"threadId":"t","target":{"type":"uncommittedChanges"}}),
        "review/start is not available through the Router: Codex review execution is not a provider-session operation",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_realtime() {
    tap_refusal(
        "refused_realtime",
        "Dispositions: Refused Realtime",
        "thread/realtime/start",
        json!({"threadId":"t","outputModality":"text"}),
        "thread/realtime/start is not available through the Router: provider sessions do not expose Codex realtime audio",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_account_limits() {
    tap_refusal(
        "refused_account_limits",
        "Dispositions: Refused AccountLimits",
        "account/rateLimits/read",
        json!({}),
        "account/rateLimits/read is not available through the Router: Codex account limits do not describe this provider account",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_provider_auth() {
    tap_refusal(
        "refused_provider_auth",
        "Dispositions: Refused ProviderAuth",
        "account/login/start",
        json!({"type":"apiKey","apiKey":"x"}),
        "account/login/start is not available through the Router: provider authentication is managed outside the Codex login flow",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_history_revert() {
    tap_refusal(
        "refused_history_revert",
        "Dispositions: Refused HistoryRevert",
        "thread/revert",
        json!({"threadId":"t","beforeTurnId":"turn-1"}),
        "thread/revert is not available through the Router: provider conversation history cannot be reverted here",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_codex_memory() {
    tap_refusal(
        "refused_codex_memory",
        "Dispositions: Refused CodexMemory",
        "thread/memory/read",
        json!({"threadId":"t"}),
        "thread/memory/read is not available through the Router: Codex memory management is not exposed by provider sessions",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_thread_goals() {
    tap_refusal(
        "refused_thread_goals",
        "Dispositions: Refused ThreadGoals",
        "thread/goal/get",
        json!({"threadId":"t"}),
        "thread/goal/get is not available through the Router: Codex thread goals are not exposed by provider sessions",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_config_write() {
    tap_refusal(
        "refused_config_write",
        "Dispositions: Refused ConfigWrite (config/write)",
        "config/write",
        json!({"keyPath":"model","value":"x","mergeStrategy":"replace"}),
        "Saving Codex configuration/defaults is not supported on this ACP endpoint",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_config_batch_write() {
    tap_refusal(
        "refused_config_batch_write",
        "Dispositions: Refused ConfigWrite (config/batchWrite)",
        "config/batchWrite",
        json!({"edits":[{"keyPath":"model","value":"x","mergeStrategy":"replace"}]}),
        "Saving Codex configuration/defaults is not supported on this ACP endpoint",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_config_trust_read_only() {
    tap_refusal(
        "refused_config_trust_read_only",
        "Dispositions: Refused ConfigTrustReadOnly",
        "config/write",
        json!({"keyPath":"projects.\"/tmp/project\".trust_level","value":"trusted","mergeStrategy":"replace"}),
        "Provider session trust is read-only here; trust this project in Codex itself",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_hook_discovery() {
    tap_refusal(
        "refused_hook_discovery",
        "Dispositions: Refused HookDiscovery",
        "hooks/list",
        json!({"cwds":[]}),
        "Hook discovery is not supported on this ACP endpoint",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_refused_permission_profile_discovery() {
    tap_refusal(
        "refused_permission_profile_discovery",
        "Dispositions: Refused PermissionProfileDiscovery",
        "permissionProfile/list",
        json!({}),
        "Permission profile discovery is not supported on this ACP endpoint",
    )
    .await;
}

#[tokio::test]
async fn node_tap_n1_minimal_compact_unsupported() {
    // Arrange: a started thread whose provider cannot compact.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let started = request_dispatch::handle_app_server_rpc(
        new_context(&backend),
        json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
    )
    .await
    .expect("start");
    let thread = started.response["result"]["thread"]["id"]
        .as_str()
        .expect("thread")
        .to_owned();
    *backend.compact_failure.lock().expect("lock") =
        Some(collaboration_contracts::CommandFailure::Unsupported);
    let issued_before = commands_seen_at_n4(&backend).len();
    let mut case = TapCase::new(
        N1,
        "minimal_compact_unsupported",
        "Dispositions: Minimal CompactUnsupported",
    );
    case.state_before(state_with_commands(issued_before));
    // Act: N1 asks N4 to compact (the provider refuses), then answers.
    dispatch_recorded(
        &mut case,
        &backend,
        json!({"id":71,"method":"thread/compact/start","params":{"threadId":thread}}),
        issued_before,
    )
    .await;
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert: catalog row "compact/start: empty result plus a turn error".
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionCommand,
            N4,
            "Compact",
            whole_command(&SessionTargetCommand {
                target: literal_session(),
                actor: literal_actor(),
            }),
        )
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "response:thread/compact/start",
            json!({"id":71,"result":{}}),
        )
        .produces_with(
            MessageFamily::CodexMessageToCli,
            EdgeParty::CodexCli,
            "error",
            json!({"method":"error","params":{
                "threadId":thread,"turnId":"compact-request:71","willRetry":false,
                "error":{"message":"Compaction is unsupported for this provider session","codexErrorInfo":null}
            }}),
        )
        .state_after(state_with_commands(issued_before + 1));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_handled_turn_start_issues_prompt_command_to_n4() {
    // Arrange: a started thread.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let started = request_dispatch::handle_app_server_rpc(
        new_context(&backend),
        json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
    )
    .await
    .expect("start");
    let thread = started.response["result"]["thread"]["id"]
        .as_str()
        .expect("thread")
        .to_owned();
    let issued_before = commands_seen_at_n4(&backend).len();
    let mut case = TapCase::new(
        N1,
        "handled_turn_start_to_n4",
        "N1 node: session commands -> N4",
    );
    case.state_before(state_with_commands(issued_before));
    // Act
    dispatch_recorded(
        &mut case,
        &backend,
        json!({"id":2,"method":"turn/start","params":{"threadId":thread,
            "input":[{"type":"text","text":"hello"}]}}),
        issued_before,
    )
    .await;
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionCommand,
            N4,
            "Prompt",
            whole_command(&PromptSessionCommand {
                target: literal_session(),
                input_id: volatile_input_id(),
                submission: literal_text_submission("hello"),
                actor: literal_actor(),
            }),
        )
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "response:turn/start",
            json!({"id":2,"result":{"turn":{
                "id":"turn-1","items":[],"status":"inProgress","error":null,
                "startedAt":null,"completedAt":null,"durationMs":null
            }}}),
        )
        .state_after(state_with_commands(issued_before + 1));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_handled_turn_interrupt_issues_interrupt_command_to_n4() {
    // Arrange
    let backend = ScriptedSessionBackend::new().expect("backend");
    let started = request_dispatch::handle_app_server_rpc(
        new_context(&backend),
        json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
    )
    .await
    .expect("start");
    let thread = started.response["result"]["thread"]["id"]
        .as_str()
        .expect("thread")
        .to_owned();
    let issued_before = commands_seen_at_n4(&backend).len();
    let mut case = TapCase::new(
        N1,
        "handled_turn_interrupt_to_n4",
        "N1 node: session commands -> N4",
    );
    case.state_before(state_with_commands(issued_before));
    // Act
    dispatch_recorded(
        &mut case,
        &backend,
        json!({"id":3,"method":"turn/interrupt","params":{"threadId":thread,"turnId":"turn-1"}}),
        issued_before,
    )
    .await;
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::SessionCommand,
            N4,
            "Interrupt",
            whole_command(&SessionTargetCommand {
                target: literal_session(),
                actor: literal_actor(),
            }),
        )
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "response:turn/interrupt",
            json!({"id":3,"result":{}}),
        )
        .state_after(state_with_commands(issued_before + 1));
    case.finish(&row).assert_settled();
}

/// Current defect (D3): a client request in the pinned contract that no
/// disposition covers is answered "method not found" (-32601).
#[tokio::test]
async fn node_tap_n1_unlisted_request_is_answered_method_not_found() {
    // Arrange: `thread/loaded/list` is a pinned ClientRequest with no handler.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let mut case = TapCase::new(
        N1,
        "unlisted_request_method_not_found",
        "Dispositions: change: no request is answered method not found",
    );
    // Act
    tap_dispatch(
        &mut case,
        &backend,
        json!({"id":9,"method":"thread/loaded/list","params":{}}),
    )
    .await;
    // Assert: target = one of the named dispositions; today = -32601.
    case.current_defect(
        CatalogRowExpectation::new()
            .produces_with(
                MessageFamily::CodexClientRequestFromCli,
                EdgeParty::CodexCli,
                "error:-32601:thread/loaded/list",
                json!({"id":9,"error":{"code":-32601,"message":"Method not found"}}),
            )
            .state_after(state_with_commands(0)),
    );
    let row = CatalogRowExpectation::new()
        .produces(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "<named disposition other than error:-32601>:thread/loaded/list",
        )
        .state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}
