//! Exercise the callable MCP Load boundary; only the external provider backend
//! is supplied. Stored-policy selection is covered separately by the source adapter.
use super::*;

#[tokio::test]
async fn mcp_provider_load_accepts_human_and_session_requesters_without_rewriting_metadata() {
    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"claude-code"});
    let target = json!({"endpoint":endpoint,"sessionId":"provider-thread"});
    let approver = json!({"humanId":"saved-approval-owner"});
    for requester in [
        json!({"humanId":"source-human"}),
        json!({"endpoint":{"serviceId":SERVICE_ID,"endpointId":"codex-local"},"sessionId":"requesting-agent"}),
    ] {
        let temporary = tempfile::tempdir().expect("temporary service directory");
        let saved_directory = temporary.path().join("saved-working-directory");
        std::fs::create_dir(&saved_directory).expect("saved directory");
        let backend = ScriptedProviderBackend::new()
            .answer("load", json!({"result":{
                "admission":"admitted", "operation":operation_snapshot(LOAD_OPERATION,"conversationLoad")
            }}))
            .answer("wait", json!({"result":{
                "operation":terminal_snapshot(LOAD_OPERATION,"conversationLoad"),
                "output":{"kind":"available","settlement":{"kind":"loaded","target":target,
                    "effectiveSettings":{"requestedPolicy":{"access":"write-restricted"},
                        "mappingStatus":"verified","authentication":"authenticated"}}}
            }}));
        let listener = start_listener(temporary.path(), &backend).await;
        let response = call_tool(
            &reqwest::Client::new(),
            &listener,
            2,
            "conversation_load",
            json!({
                "operationId":LOAD_OPERATION, "target":target, "generation":generation(),
                "workingDirectory":saved_directory, "requestedBy":requester,
                "approver":approver, "access":"write-restricted"
            }),
        )
        .await;

        assert_eq!(
            response["result"]["isError"], false,
            "Human and Session are both established provider requester identities: {response}"
        );
        let result = &response["result"]["structuredContent"];
        assert_eq!(result["kind"], "completed", "{response}");
        assert_eq!(result["operationId"], LOAD_OPERATION);
        assert_eq!(result["target"], target);
        assert_eq!(result["settlement"]["detail"]["kind"], "providerLoad");
        let calls = backend.calls();
        assert_eq!(
            calls.len(),
            2,
            "one Load and one settlement wait: {calls:?}"
        );
        assert_eq!(calls[0].0, "load");
        let request = &calls[0].1;
        assert_eq!(request["requestedBy"], requester);
        assert_eq!(request["approver"], approver);
        assert_eq!(request["workingDirectory"], json!(saved_directory));
        assert_eq!(
            request["requestedPolicy"],
            json!({"access":"write-restricted"})
        );
        assert_eq!(request["target"], target);
        assert_eq!(request["operationId"], LOAD_OPERATION);
        assert_eq!(request["generation"], generation());
        listener.stop().await;
    }
}

#[tokio::test]
async fn mcp_provider_load_rejects_foreign_session_requester_or_approver_before_backend_dispatch() {
    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"claude-code"});
    let target = json!({"endpoint":endpoint,"sessionId":"provider-thread"});
    let local_actor = json!({"endpoint":endpoint,"sessionId":"requesting-agent"});
    let foreign_actor = json!({"endpoint":{
        "serviceId":"00000000-0000-4000-8000-000000000009","endpointId":"codex-local"
    },"sessionId":"foreign-agent"});
    for (requester, approver) in [
        (foreign_actor.clone(), local_actor),
        (json!({"humanId":"source-human"}), foreign_actor),
    ] {
        let temporary = tempfile::tempdir().expect("temporary service directory");
        let backend = ScriptedProviderBackend::new();
        let listener = start_listener(temporary.path(), &backend).await;
        let response = call_tool(
            &reqwest::Client::new(),
            &listener,
            2,
            "conversation_load",
            json!({
                "operationId":LOAD_OPERATION,"target":target,"workingDirectory":temporary.path(),
                "requestedBy":requester,"approver":approver,"access":"write-restricted"
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            response
                .to_string()
                .contains("requester and approver must belong to the target service"),
            "{response}"
        );
        assert!(
            backend.calls().is_empty(),
            "foreign Session identity must fail before effects"
        );
        listener.stop().await;
    }
}
