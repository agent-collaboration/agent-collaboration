use super::*;

#[test]
fn endpoint_startup_reason_preserves_sanitized_failure_metadata() {
    let account_sentinel = "synthetic-account-sentinel@example.invalid";
    let token_sentinel = "synthetic-token-sentinel-7f4e";
    let source_error =
        agent_client_protocol::Error::new(-32001, format!("denied {token_sentinel}"))
            .data(serde_json::json!({"account": account_sentinel, "token": token_sentinel}));
    let runtime_error = crate::ExternalProviderRuntimeError::Initialize(
        acp_client_runtime::sanitized_initialization_error_for_test(&source_error),
    );

    let reason = provider_startup_failure_reason("/fixture/acp-provider", &runtime_error);

    assert!(reason.contains("initialize"));
    assert!(reason.contains("-32001"));
    assert!(reason.contains("error_data_bytes="));
    assert!(!reason.contains(account_sentinel));
    assert!(!reason.contains(token_sentinel));
}
