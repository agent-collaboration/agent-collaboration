//! Public MCP requester decoding and its actual application request projection.
use super::*;

#[tokio::test]
async fn human_prompt_and_cancel_keep_the_callers_stale_generation_refusal() {
    // Arrange: the binding is generation 3, while the caller deliberately guards generation 2.
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new();
    let listener = start_listener(temporary.path(), &backend).await;
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    for name in ["conversation_prompt", "conversation_cancel"] {
        let mut arguments = json!({"operationId":CANCEL_OPERATION,"targetOperationId":PROMPT_OPERATION,
            "target":target,"generation":{"serviceEpoch":SERVICE_EPOCH,"generation":2},
            "requestedBy":{"kind":"human","humanId":"fixture-operator"},
            "approver":{"humanId":"independent-approver"}});
        if name == "conversation_prompt" {
            arguments
                .as_object_mut()
                .expect("arguments")
                .remove("targetOperationId");
            arguments["message"] = json!({"kind":"humanUser","text":"must not dispatch"});
        }
        // Act
        let response = call_tool(&reqwest::Client::new(), &listener, 2, name, arguments).await;
        // Assert: dropping the guard would reach the backend, so a matching positive guard cannot mask it.
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert_eq!(
            response["result"]["structuredContent"]["kind"], "staleGeneration",
            "{response}"
        );
        assert_eq!(
            response["result"]["structuredContent"]["operationId"],
            CANCEL_OPERATION
        );
        assert_eq!(response["result"]["structuredContent"]["effect"], "none");
    }
    assert!(backend.calls().is_empty());
    listener.stop().await;
}

#[tokio::test]
async fn discovered_prompt_and_cancel_schemas_reuse_the_creation_actor_contract() {
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new();
    let listener = start_listener(temporary.path(), &backend).await;
    let discovered = call_mcp(
        &reqwest::Client::new(),
        &listener,
        2,
        "tools/list",
        json!({}),
    )
    .await;
    let tools = discovered["result"]["tools"].as_array().expect("tools");
    let schema = |name: &str| {
        &tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("conversation tool")["inputSchema"]
    };
    let create = schema("conversation_create");
    assert!(!create["properties"]["createdBy"].is_null());
    for name in ["conversation_prompt", "conversation_cancel"] {
        let control = schema(name);
        assert_eq!(
            control["properties"]["requestedBy"], create["properties"]["createdBy"],
            "{name}"
        );
        assert_eq!(
            control["$defs"]["ConversationActor"], create["$defs"]["ConversationActor"],
            "{name}"
        );
    }
    assert!(backend.calls().is_empty());
    listener.stop().await;
}

#[tokio::test]
async fn foreign_requester_or_approver_is_rejected_before_prompt_or_cancel_dispatch() {
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new();
    let listener = start_listener(temporary.path(), &backend).await;
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    let foreign = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000009",
        "endpointId":"codex-local"},"sessionId":"foreign-agent"});
    for (requester, approver) in [
        (
            json!({"kind":"session","session":foreign}),
            json!({"humanId":"fixture-approver"}),
        ),
        (
            json!({"kind":"human","humanId":"fixture-operator"}),
            foreign,
        ),
    ] {
        for name in ["conversation_prompt", "conversation_cancel"] {
            let mut arguments = json!({"operationId":CANCEL_OPERATION,"targetOperationId":PROMPT_OPERATION,
                "target":target,"requestedBy":requester,"approver":approver});
            if name == "conversation_prompt" {
                arguments
                    .as_object_mut()
                    .expect("arguments")
                    .remove("targetOperationId");
                arguments["message"] = json!({"kind":"humanUser","text":"must not dispatch"});
            }
            let response = call_tool(&reqwest::Client::new(), &listener, 2, name, arguments).await;
            assert_eq!(response["result"]["isError"], true, "{response}");
            assert_eq!(
                response["result"]["structuredContent"]["kind"], "invalidRequest",
                "{response}"
            );
            assert_eq!(response["result"]["structuredContent"]["effect"], "none");
            assert_eq!(
                response["result"]["structuredContent"]["message"],
                "requester and approver must belong to the target service"
            );
        }
    }
    assert!(backend.calls().is_empty());
    listener.stop().await;
}

#[tokio::test]
async fn malformed_typed_requesters_never_reach_provider_operations() {
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new();
    let listener = start_listener(temporary.path(), &backend).await;
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    let session = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"codex-local"},"sessionId":"fixture-agent"});
    for requester in [
        json!({"kind":"human","humanId":""}),
        json!({"kind":"human","humanId":"owner","session":session}),
        json!({"kind":"session","session":{}}),
        json!({"kind":"unknown","endpoint":session["endpoint"],"sessionId":"fixture-agent"}),
    ] {
        for name in ["conversation_prompt", "conversation_cancel"] {
            let mut arguments = json!({"operationId":CANCEL_OPERATION,
                "targetOperationId":PROMPT_OPERATION,"target":target,"requestedBy":requester});
            if name == "conversation_prompt" {
                arguments
                    .as_object_mut()
                    .expect("arguments")
                    .remove("targetOperationId");
                arguments["message"] = json!({"kind":"humanUser","text":"must not dispatch"});
            }
            let response = call_tool(&reqwest::Client::new(), &listener, 2, name, arguments).await;
            assert_eq!(response["result"]["isError"], true, "{response}");
        }
    }
    assert!(
        backend.calls().is_empty(),
        "malformed typed identity dispatched"
    );
    listener.stop().await;
}

#[tokio::test]
async fn human_composite_retains_created_target_when_prompt_is_rejected() {
    let temporary = tempfile::tempdir().expect("service directory");
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    let backend = ScriptedProviderBackend::new()
        .answer(
            "create",
            json!({"result":{"admission":"admitted",
            "operation":operation_snapshot(CREATE_OPERATION,"conversationCreate")}}),
        )
        .answer(
            "wait",
            json!({"result":{
                "operation":terminal_snapshot(CREATE_OPERATION,"conversationCreate"),
                "output":{"kind":"available","settlement":{"kind":"created","target":target,
                    "effectiveSettings":{"requestedPolicy":{"access":"workspace-write"},
                        "mappingStatus":"verified","authentication":"authenticated"}}}
            }}),
        )
        .answer(
            "prompt",
            json!({"error":{"kind":"busy","stage":"admission","effect":"none",
            "message":"held provider boundary","operationId":PROMPT_OPERATION,"target":target}}),
        );
    let listener = start_listener(temporary.path(), &backend).await;
    let response = call_tool(&reqwest::Client::new(), &listener, 2, "conversation_create_and_prompt", json!({
        "create":{"operationId":CREATE_OPERATION,"endpoint":target["endpoint"],
            "workingDirectory":temporary.path(),"generation":generation(),"access":"workspace-write",
            "createdBy":{"kind":"human","humanId":"fixture-operator"},
            "approver":{"kind":"human","humanId":"independent-approver"}},
        "promptOperationId":PROMPT_OPERATION,"message":{"kind":"humanUser","text":"controlled composite"}
    })).await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    let failure = &response["result"]["structuredContent"];
    assert_eq!(failure["kind"], "busy", "{response}");
    assert_eq!(failure["createOperationId"], CREATE_OPERATION);
    assert_eq!(failure["operationId"], PROMPT_OPERATION);
    assert_eq!(failure["target"], target);
    let calls = backend.calls();
    assert_eq!(
        calls
            .iter()
            .map(|(operation, _)| *operation)
            .collect::<Vec<_>>(),
        ["create", "wait", "prompt"]
    );
    assert_eq!(calls[0].1["operationId"], CREATE_OPERATION);
    assert_eq!(calls[2].1["operationId"], PROMPT_OPERATION);
    assert_eq!(calls[2].1["generation"], generation());
    assert_eq!(
        calls[2].1["requestedBy"],
        json!({"humanId":"fixture-operator"})
    );
    assert_eq!(
        calls[2].1["approver"],
        json!({"humanId":"independent-approver"})
    );
    listener.stop().await;
}

#[tokio::test]
async fn human_prompt_preserves_requester_approver_generation_and_operation() {
    // Arrange: only the external provider is scripted; MCP and carrier decoding stay real.
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new()
        .answer("prompt", json!({"result":{"admission":"admitted",
            "operation":operation_snapshot(PROMPT_OPERATION,"conversationPrompt")}}))
        .answer("wait", json!({"result":{
            "operation":terminal_snapshot(PROMPT_OPERATION,"conversationPrompt"),
            "output":{"kind":"available","settlement":{"kind":"promptCompleted",
                "target":{"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"},
                "stopReason":"end_turn","response":"controlled human reply"}}
        }}));
    let listener = start_listener(temporary.path(), &backend).await;
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    let approver = json!({"humanId":"independent-approver"});

    // Act: the operator uses its existing typed Human identity and HumanUser content.
    let response = call_tool(
        &reqwest::Client::new(),
        &listener,
        2,
        "conversation_prompt",
        json!({
            "operationId":PROMPT_OPERATION,"target":target,"generation":generation(),
            "requestedBy":{"kind":"human","humanId":"fixture-operator"},"approver":approver,
            "message":{"kind":"humanUser","text":"controlled prompt"}
        }),
    )
    .await;

    // Assert: no borrowed Session and no metadata lost between the public producer/consumer.
    assert_eq!(response["result"]["isError"], false, "{response}");
    let result = &response["result"]["structuredContent"];
    assert_eq!(result["operationId"], PROMPT_OPERATION);
    assert_eq!(result["target"], target);
    assert_eq!(
        result["settlement"]["detail"]["output"]["text"],
        "controlled human reply"
    );
    let calls = backend.calls();
    assert_eq!(
        calls
            .iter()
            .map(|(operation, _)| *operation)
            .collect::<Vec<_>>(),
        ["prompt", "wait"]
    );
    assert_eq!(
        calls[0].1["requestedBy"],
        json!({"humanId":"fixture-operator"})
    );
    assert_eq!(calls[0].1["approver"], approver);
    assert_eq!(calls[0].1["generation"], generation());
    assert_eq!(calls[0].1["operationId"], PROMPT_OPERATION);
    assert_eq!(calls[0].1["target"], target);
    listener.stop().await;
}

#[tokio::test]
async fn human_cancel_preserves_exact_operation_and_independent_approver() {
    // Arrange
    let temporary = tempfile::tempdir().expect("service directory");
    let backend = ScriptedProviderBackend::new().answer("cancel", json!({"result":{
        "admission":"admitted","operation":operation_snapshot(CANCEL_OPERATION,"conversationCancel")
    }}));
    let listener = start_listener(temporary.path(), &backend).await;
    let target = json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"});
    let approver = json!({"humanId":"independent-approver"});

    // Act
    let response = call_tool(
        &reqwest::Client::new(),
        &listener,
        2,
        "conversation_cancel",
        json!({
            "operationId":CANCEL_OPERATION,"targetOperationId":PROMPT_OPERATION,"target":target,
            "generation":generation(),"requestedBy":{"kind":"human","humanId":"fixture-operator"},
            "approver":approver
        }),
    )
    .await;

    // Assert
    assert_eq!(response["result"]["isError"], false, "{response}");
    let calls = backend.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "cancel");
    assert_eq!(calls[0].1["operationId"], CANCEL_OPERATION);
    assert_eq!(calls[0].1["targetOperationId"], PROMPT_OPERATION);
    assert_eq!(calls[0].1["target"], target);
    assert_eq!(
        calls[0].1["requestedBy"],
        json!({"humanId":"fixture-operator"})
    );
    assert_eq!(calls[0].1["approver"], approver);
    assert_eq!(calls[0].1["generation"], generation());
    listener.stop().await;
}
