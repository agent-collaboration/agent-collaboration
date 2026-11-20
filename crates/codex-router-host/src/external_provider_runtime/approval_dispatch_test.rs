use super::*;
use acp_scripted_fixture::AcpFixtureScript;
use serde_json::{Value, json};

fn interaction_script(
    method: &str,
    params: Value,
    refusal: Value,
    replay: bool,
) -> AcpFixtureScript {
    let callback = json!({"jsonrpc":"2.0","id":"callback","method":method,"params":params});
    let mut script = ready_script()
        .expect_request("first", "session/prompt", json!({"sessionId":"session-a"}))
        .send(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"session-a","update":{
                "sessionUpdate":"tool_call","toolCallId":"interaction-tool","title":"Interaction",
                "kind":"other","status":"pending"
            }
        }}))
        .send(callback.clone())
        .expect_message(json!({"jsonrpc":"2.0","id":"callback","result":refusal}))
        .expect_message(
            json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"session-a"}}),
        );
    if replay {
        script = replay_then_sync(script, &callback, "first-sync");
    }
    script = script
        .respond("first", json!({"stopReason":"cancelled"}))
        .expect_request("next", "session/prompt", json!({"sessionId":"session-a"}));
    if replay {
        script = replay_then_sync(script, &callback, "next-sync");
    }
    script
        .send(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"session-a","update":{
                "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"next reply"}
            }
        }}))
        .respond("next", json!({"stopReason":"end_turn"}))
}

fn ready_script() -> AcpFixtureScript {
    AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond(
            "initialize",
            json!({
                "protocolVersion":1, "agentCapabilities":{},
                "agentInfo":{"name":"managed-refusal-fixture","version":"1"}
            }),
        )
        .expect_request("create", "session/new", json!({}))
        .respond("create", json!({"sessionId":"session-a"}))
}

async fn start_scripted_provider(script: AcpFixtureScript) -> ExternalProviderRuntime {
    let runtime = ExternalProviderRuntime::initialize(
        script.launch(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("managed provider initialization");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session creation");
    runtime
}

fn replay_then_sync(script: AcpFixtureScript, callback: &Value, sync_id: &str) -> AcpFixtureScript {
    // The reply to the following request keeps each replay on its own side
    // of the first turn's terminal, with no elapsed-time assumption.
    script
        .send(callback.clone())
        .send(json!({"jsonrpc":"2.0","id":sync_id,"method":"fixture/sync","params":{}}))
        .expect_message(json!({"jsonrpc":"2.0","id":sync_id,
            "error":{"code":-32601,"message":"Method not found"}}))
}

async fn exercise_managed_acp_callbacks(replay: bool) {
    for (method, params, refusal) in [
        (
            "session/request_permission",
            json!({"sessionId":"session-a","toolCall":{"toolCallId":"interaction-tool","title":"Run"},
                "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]}),
            json!({"outcome":{"outcome":"cancelled"}}),
        ),
        (
            "cursor/ask_question",
            json!({"toolCallId":"interaction-tool","title":"Choose","questions":[
                {"id":"choice","prompt":"Proceed?","options":[{"id":"yes","label":"Yes"}]}
            ]}),
            json!({"outcome":{"outcome":"cancelled"}}),
        ),
        (
            "elicitation/create",
            json!({"mode":"form","sessionId":"session-a","message":"Confirm",
                "requestedSchema":{"type":"object","properties":{"answer":{"type":"string"}}}}),
            json!({"action":"cancel"}),
        ),
        (
            "cursor/create_plan",
            json!({"toolCallId":"interaction-tool","name":"Plan","overview":"Overview","plan":"# Plan",
                "todos":[{"id":"one","content":"Step","status":"pending"}]}),
            json!({"outcome":"cancelled"}),
        ),
    ] {
        tokio::time::timeout(Duration::from_secs(10), async {
            let runtime =
                start_scripted_provider(interaction_script(method, params, refusal, replay)).await;
            let first = runtime
                .prompt("session-a".into(), "request".into())
                .await
                .expect("provider terminal after callback refusal");
            assert_eq!(
                first.stop_reason,
                ProviderPromptStopReason::Cancelled,
                "{method}"
            );
            if method == "session/request_permission" {
                assert_eq!(
                    first
                        .permission_refusal_reason
                        .map(ExternalProviderApprovalRefusalReason::code),
                    Some("approvalsUnavailableOnBuild"),
                );
            }
            let next = runtime
                .prompt("session-a".into(), "next".into())
                .await
                .expect("next prompt on the same session");
            assert_eq!(next.stop_reason, ProviderPromptStopReason::EndTurn);
            assert_eq!(next.output, "next reply");
            runtime.shutdown().await;
        })
        .await
        .unwrap_or_else(|error| panic!("{method} did not settle: {error}"));
    }
}

#[tokio::test]
async fn managed_acp_callbacks_refuse_before_cancel_and_next_prompt_succeeds() {
    exercise_managed_acp_callbacks(false).await;
}

#[tokio::test]
async fn managed_acp_replay_cannot_reply_twice_or_cancel_the_next_operation() {
    exercise_managed_acp_callbacks(true).await;
}

#[tokio::test]
async fn managed_acp_distinct_request_reusing_finished_id_is_refused_and_cancelled() {
    let root = tempfile::tempdir().expect("ACP fixture directory");
    let reused_sent = root.path().join("reused-request-sent");
    let permission_request = |tool_call_id| {
        json!({"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{
            "sessionId":"session-a","toolCall":{"toolCallId":tool_call_id,"title":"Run"},
            "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]
        }})
    };
    let reply = json!({"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}});
    let cancel =
        json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"session-a"}});
    let script = ready_script()
        .expect_request("first", "session/prompt", json!({"sessionId":"session-a"}))
        .send(permission_request("first-tool"))
        .expect_message(reply.clone())
        .expect_message(cancel.clone())
        .respond("first", json!({"stopReason":"cancelled"}))
        .expect_request("second", "session/prompt", json!({"sessionId":"session-a"}))
        .send(permission_request("second-tool"))
        .write_marker(&reused_sent)
        .expect_message(reply)
        .expect_message(cancel)
        .respond("second", json!({"stopReason":"cancelled"}));
    let runtime = start_scripted_provider(script).await;

    let first = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.prompt("session-a".into(), "first".into()),
    )
    .await
    .expect("the first callback gets a reply and exact cancel")
    .expect("first provider terminal");
    assert_eq!(first.stop_reason, ProviderPromptStopReason::Cancelled);
    let second = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.prompt("session-a".into(), "second".into()),
    )
    .await;
    runtime.shutdown().await;

    assert!(
        reused_sent.is_file(),
        "the provider sent its second callback"
    );
    let second = second
        .expect("the distinct callback reusing ID 7 got no refusal and exact cancel")
        .expect("second provider terminal");
    assert_eq!(second.stop_reason, ProviderPromptStopReason::Cancelled);
    assert_eq!(
        second
            .permission_refusal_reason
            .map(ExternalProviderApprovalRefusalReason::code),
        Some("approvalsUnavailableOnBuild")
    );
}

async fn assert_refusal_before_answer_model(method: &str, params: Value, refusal: Value) {
    let root = tempfile::tempdir().expect("ACP fixture directory");
    let diagnostics = root.path().join("diagnostics");
    let script =
        interaction_script(method, params, refusal, false).record_diagnostics(diagnostics.clone());
    let runtime = start_scripted_provider(script).await;

    let first = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.prompt("session-a".into(), "request".into()),
    )
    .await;
    let first = first
        .expect("provider terminal after refusal")
        .unwrap_or_else(|error| {
            panic!(
                "{method} received no valid refusal and exact cancel: {error:?}\n{}",
                std::fs::read_to_string(&diagnostics).unwrap_or_default()
            )
        });
    assert_eq!(first.stop_reason, ProviderPromptStopReason::Cancelled);
    let next = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.prompt("session-a".into(), "next".into()),
    )
    .await
    .expect("next prompt settles")
    .expect("next provider terminal");
    assert_eq!(next.stop_reason, ProviderPromptStopReason::EndTurn);
    assert_eq!(next.output, "next reply");
    runtime.shutdown().await;
}

#[tokio::test]
async fn managed_acp_empty_form_is_refused_and_cancelled_before_answer_model() {
    assert_refusal_before_answer_model(
        "elicitation/create",
        json!({"mode":"form","sessionId":"session-a","message":"Confirm",
            "requestedSchema":{"type":"object","properties":{}}}),
        json!({"action":"cancel"}),
    )
    .await;
}

#[tokio::test]
async fn managed_cursor_question_is_refused_and_cancelled_before_answer_model() {
    assert_refusal_before_answer_model(
        "cursor/ask_question",
        json!({"toolCallId":"interaction-tool","title":"Choose","questions":[
            {"id":"choice","prompt":"Proceed?","options":[]}
        ]}),
        json!({"outcome":{"outcome":"cancelled"}}),
    )
    .await;
}
