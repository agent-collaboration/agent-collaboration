//! Unsupported native operations retain an actionable error through RPC dispatch.
use super::test_support::ScriptedSessionBackend;
use super::*;

// The pinned Codex TUI preserves server error messages on these request paths:
// app_server_session.rs, app/thread_routing.rs, app_server_session/realtime.rs,
// and workspace_command.rs at upstream 360319d8.
#[tokio::test]
async fn unavailable_native_operations_explain_the_router_boundary() {
    // Arrange: the real request dispatcher, with no provider operations allowed.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor().expect("actor"),
        backend.clone(),
        backend.clone(),
        test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let methods = [
        "command/exec",
        "command/exec/write",
        "command/exec/terminate",
        "command/exec/resize",
        "thread/fork",
        "thread/inject_items",
        "thread/name/set",
        "thread/archive",
        "thread/unarchive",
        "thread/shellCommand",
        "thread/backgroundTerminals/clean",
        "thread/backgroundTerminals/list",
        "thread/backgroundTerminals/terminate",
        "thread/queue/add",
        "thread/queue/list",
        "thread/queue/update",
        "thread/queue/delete",
        "thread/queue/reorder",
        "thread/queue/start",
        "review/start",
        "thread/realtime/start",
        "thread/realtime/appendAudio",
        "thread/realtime/appendText",
        "thread/realtime/appendSpeech",
        "thread/realtime/stop",
        "thread/realtime/listVoices",
        "account/rateLimits/read",
        "account/login/start",
        "account/login/cancel",
        "account/logout",
        "thread/revert",
        "thread/memoryMode/set",
        "thread/memory/read",
        "memory/status",
        "memory/reset",
        "thread/goal/set",
        "thread/goal/get",
        "thread/goal/clear",
    ];
    let mut failures = Vec::new();
    for method in methods {
        // Act: dispatch the same method name the native client sends.
        let handled = request_dispatch::handle_app_server_rpc(
            context.clone(),
            json!({"id":71,"method":method,"params":{"threadId":"unused"}}),
        )
        .await
        .expect("dispatch");

        // Assert: a known unsupported request is a visible operation error,
        // with its request identity and a reason, never a fake success.
        let response = handled.response;
        let message = response["error"]["message"].as_str().unwrap_or_default();
        if response["id"] != 71
            || response["error"]["code"] != -32000
            || !message.starts_with(&format!("{method} is not available through the Router: "))
            || message.ends_with(": ")
            || response.get("result").is_some()
            || !handled.post_response_frames.is_empty()
        {
            failures.push((method, response));
        }
    }
    assert!(failures.is_empty(), "unhelpful native errors: {failures:?}");
    assert!(backend.create_commands.lock().expect("commands").is_empty());
    assert!(backend.prompt_commands.lock().expect("commands").is_empty());
    assert!(backend.cancel_commands.lock().expect("commands").is_empty());
    assert!(
        backend
            .setting_commands
            .lock()
            .expect("commands")
            .is_empty()
    );
}

#[test]
fn unknown_native_method_remains_method_not_found() {
    // Arrange / Act: a name that is not a known native operation.
    let response = handle_app_server_request(
        json!(72),
        "router/not-a-native-method",
        &json!({}),
        &[],
        None,
    );
    // Assert: this must not pretend to recognize arbitrary methods.
    assert_eq!(
        response,
        json!({"id":72,"error":{"code":-32601,"message":"Method not found"}})
    );
}
