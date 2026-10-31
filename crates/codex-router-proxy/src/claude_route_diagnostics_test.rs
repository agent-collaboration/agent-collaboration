use super::*;

#[test]
fn producer_wire_is_literal_finite_and_rejects_unknown_or_secret_shaped_causes() {
    let mut output = Vec::new();
    for event in [
        ClaudeRouteDiagnostic::Arrival,
        ClaudeRouteDiagnostic::UnsupportedRoute,
        ClaudeRouteDiagnostic::AttemptSelected,
        ClaudeRouteDiagnostic::UpstreamSend,
        ClaudeRouteDiagnostic::BodyBufferFailure,
        ClaudeRouteDiagnostic::UpstreamPreparationFailure,
        ClaudeRouteDiagnostic::LocalFailure("credential_unavailable"),
        ClaudeRouteDiagnostic::Outcome("connection_failure"),
        ClaudeRouteDiagnostic::Outcome("provider_server_error"),
        ClaudeRouteDiagnostic::Outcome("success"),
        ClaudeRouteDiagnostic::Outcome("not_attempted"),
        ClaudeRouteDiagnostic::LocalFailure("credential_unavailable token=SECRET_CANARY"),
        ClaudeRouteDiagnostic::Outcome("SECRET_CANARY"),
    ] {
        write_to(&mut output, event);
    }
    let expected = concat!(
        "codex-router claude route diagnostic: stage=arrival cause=none\n",
        "codex-router claude route diagnostic: stage=unsupported_route cause=none\n",
        "codex-router claude route diagnostic: stage=attempt_selected cause=none\n",
        "codex-router claude route diagnostic: stage=upstream_send cause=none\n",
        "codex-router claude route diagnostic: stage=body_buffer_failure cause=request_rejected\n",
        "codex-router claude route diagnostic: stage=upstream_preparation_failure cause=request_rejected\n",
        "codex-router claude route diagnostic: stage=local_failure cause=credential_unavailable\n",
        "codex-router claude route diagnostic: stage=outcome cause=connection_failure\n",
        "codex-router claude route diagnostic: stage=outcome cause=provider_server_error\n",
        "codex-router claude route diagnostic: stage=outcome cause=success\n",
    );
    assert_eq!(
        output,
        if cfg!(debug_assertions) {
            expected.as_bytes()
        } else {
            &[]
        }
    );
}

#[test]
fn failed_diagnostic_write_is_not_a_routing_error_or_panic() {
    struct BrokenSink;
    impl std::io::Write for BrokenSink {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    write_to(&mut BrokenSink, ClaudeRouteDiagnostic::Arrival);
}
