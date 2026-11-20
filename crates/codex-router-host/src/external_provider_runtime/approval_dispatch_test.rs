use super::*;
use acp_scripted_fixture::AcpFixtureScript;
use serde_json::{Value, json};

fn interaction_script(method: &str, params: Value, refusal: Value) -> AcpFixtureScript {
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
        .expect_request("first", "session/prompt", json!({"sessionId":"session-a"}))
        .send(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"session-a","update":{
                "sessionUpdate":"tool_call","toolCallId":"interaction-tool","title":"Interaction",
                "kind":"other","status":"pending"
            }
        }}))
        .send(json!({"jsonrpc":"2.0","id":"callback","method":method,"params":params}))
        .expect_message(json!({"jsonrpc":"2.0","id":"callback","result":refusal}))
        .expect_message(
            json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"session-a"}}),
        )
        .respond("first", json!({"stopReason":"cancelled"}))
        .expect_request("next", "session/prompt", json!({"sessionId":"session-a"}))
        .send(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"session-a","update":{
                "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"next reply"}
            }
        }}))
        .respond("next", json!({"stopReason":"end_turn"}))
}

#[tokio::test]
async fn managed_acp_callbacks_refuse_before_cancel_and_next_prompt_succeeds() {
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
            let runtime = ExternalProviderRuntime::initialize(
                interaction_script(method, params, refusal).launch(),
                Arc::new(acp_provider_adapters::StandardAcpBehavior),
            )
            .await
            .expect("managed provider initialization");
            runtime
                .create_session(PathBuf::from("/tmp"))
                .await
                .expect("session creation");
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
