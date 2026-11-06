//! Lifecycle refusals retain the provider's typed fields through the actual MCP HTTP path.
use super::*;
use collaboration_protocol::OperationId;

#[tokio::test]
async fn lifecycle_http_preserves_typed_refusal_fields_and_uncertainty() {
    let mut mismatches = Vec::new();
    for operation in ["resume", "close"] {
        for effect in ["none", "unknown"] {
            // Arrange the established HTTP application fixture and its provider response.
            let temporary = tempfile::tempdir().expect("temporary service directory");
            let operation_id = OperationId::generate();
            let target =
                json!({"endpoint":provider_binding()["endpoint"],"sessionId":"provider-thread"});
            let failure = json!({
                "kind":"unsupportedCapability","stage":"binding","effect":effect,
                "message":format!("provider runtime does not advertise conversation {operation}"),
                "operationId":operation_id,"target":target
            });
            let backend =
                ScriptedProviderBackend::new().answer(operation, json!({"error":failure}));
            let listener = start_listener(temporary.path(), &backend).await;
            let mut request = json!({"operationId":operation_id,"target":target,
                "requestedBy":{"humanId":"fixture-operator"},"approver":{"humanId":"fixture-operator"}});
            if operation == "resume" {
                request["workingDirectory"] = json!(temporary.path());
                request["requestedPolicy"] = json!({"access":"workspace-write"});
            }
            // Act through the real stateless HTTP route, tool, application and converter.
            let result = call_tool(
                &reqwest::Client::new(),
                &listener,
                2,
                &format!("conversation_{operation}"),
                request,
            )
            .await;
            listener.stop().await;
            // Assert all typed fields are top-level and unknown is never forced to none.
            assert_eq!(
                result["result"]["isError"], true,
                "{operation}/{effect}: {result}"
            );
            let mut expected = failure;
            expected["mcpResult"] = json!("error");
            let actual = &result["result"]["structuredContent"];
            if actual != &expected {
                mismatches.push(format!(
                    "{operation}/{effect}: actual {actual}; expected {expected}"
                ));
            }
            assert_eq!(backend.calls().len(), 1);
            assert_eq!(backend.held_operations(), (0, 0));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
