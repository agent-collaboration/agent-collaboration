//! Lifecycle refusal projection at the actual MCP tool boundary.
use super::*;
use collaboration_protocol::OperationId;
use serde_json::{Value, json};
use std::sync::Arc;

// Reuse the established application backend fixture; only its external responses are scripted.
#[path = "../provider_conversation_http_tests/scripted_provider_backend.rs"]
mod scripted_provider_backend;

fn provider_binding() -> Value {
    json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-code"},
        "bindingId":"binding-1",
        "runtime":{"provider":"claudeCode","runtimeName":"claude-agent-acp"},
        "transport":"stdioAcp",
        "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":3},
        "capabilities":[{"name":"prompt","status":"supported","evidence":"advertised"}]
    })
}

#[tokio::test]
async fn lifecycle_tools_preserve_typed_refusal_fields_and_uncertainty() {
    let mut mismatches = Vec::new();
    for operation in ["resume", "close"] {
        for effect in ["none", "unknown"] {
            // Arrange a real service rejection with a typed provider failure.
            let operation_id = OperationId::generate();
            let target =
                json!({"endpoint":provider_binding()["endpoint"],"sessionId":"provider-thread"});
            let failure = json!({
                "kind":"unsupportedCapability","stage":"binding","effect":effect,
                "message":format!("provider runtime does not advertise conversation {operation}"),
                "operationId":operation_id,"target":target
            });
            let backend = scripted_provider_backend::ScriptedProviderBackend::new()
                .answer(operation, json!({"error":failure}));
            let application = crate::api_test_harness::test_identity()
                .with_provider_conversation_backend(Arc::new(backend.clone()))
                .build();
            let server = CollaborationMcpServer::for_application(application, std::env::temp_dir());
            let mut request = json!({"operationId":operation_id,"target":target,
                "requestedBy":{"humanId":"fixture-operator"},"approver":{"humanId":"fixture-operator"}});
            if operation == "resume" {
                request["workingDirectory"] = json!(std::env::temp_dir());
                request["requestedPolicy"] = json!({"access":"workspace-write"});
            }
            // Act through the tool's real application call and result conversion.
            let result = if operation == "resume" {
                server
                    .conversation_resume(Parameters(serde_json::from_value(request).unwrap()))
                    .await
            } else {
                server
                    .conversation_close(Parameters(serde_json::from_value(request).unwrap()))
                    .await
            };
            // Assert every service field is top-level, including unknown rather than forced none.
            assert_eq!(result.is_error, Some(true));
            let mut expected = failure;
            expected["mcpResult"] = json!("error");
            let actual = result.structured_content.unwrap();
            if actual != expected {
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

#[test]
fn untyped_lifecycle_rejection_keeps_unknown_fallback() {
    let rejection = collaboration_service::collaboration_application::ConversationFailure::InvalidStoredOperation;
    let result = typed_failure_result::<collaboration_protocol::ConversationOperationFailure, (), _>(
        Err(rejection),
        OperationEffect::Unknown,
    );
    assert_eq!(result.structured_content.unwrap()["effect"], "unknown");
}
