//! Debug-only, closed Claude route observations over the existing child stderr pipe.

pub(crate) enum ClaudeRouteDiagnostic {
    Arrival,
    UnsupportedRoute,
    AttemptSelected,
    UpstreamSend,
    BodyBufferFailure,
    UpstreamPreparationFailure,
    LocalFailure(&'static str),
    Outcome(&'static str),
}

pub(crate) fn record(event: ClaudeRouteDiagnostic) {
    #[cfg(debug_assertions)]
    write_to(&mut std::io::stderr().lock(), event);
    #[cfg(not(debug_assertions))]
    match event {
        ClaudeRouteDiagnostic::LocalFailure(cause) | ClaudeRouteDiagnostic::Outcome(cause) => {
            let _unused = cause;
        }
        _ => {}
    }
}

#[cfg(any(debug_assertions, test))]
fn fields(event: ClaudeRouteDiagnostic) -> Option<(&'static str, &'static str)> {
    match event {
        ClaudeRouteDiagnostic::Arrival => Some(("arrival", "none")),
        ClaudeRouteDiagnostic::UnsupportedRoute => Some(("unsupported_route", "none")),
        ClaudeRouteDiagnostic::AttemptSelected => Some(("attempt_selected", "none")),
        ClaudeRouteDiagnostic::UpstreamSend => Some(("upstream_send", "none")),
        ClaudeRouteDiagnostic::BodyBufferFailure => {
            Some(("body_buffer_failure", "request_rejected"))
        }
        ClaudeRouteDiagnostic::UpstreamPreparationFailure => {
            Some(("upstream_preparation_failure", "request_rejected"))
        }
        ClaudeRouteDiagnostic::LocalFailure(cause) => match cause {
            "local_auth_rejected"
            | "request_rejected"
            | "upstream_failure"
            | "credential_unavailable"
            | "no_openai_accounts_configured"
            | "no_eligible_account"
            | "short_quota_exhausted"
            | "selection_state_unavailable"
            | "selector_state_unavailable"
            | "affinity_secret_unavailable"
            | "malformed_affinity_key"
            | "affinity_owner_missing"
            | "affinity_owner_unavailable" => Some(("local_failure", cause)),
            _ => None,
        },
        ClaudeRouteDiagnostic::Outcome(cause) => match cause {
            "success"
            | "shared_window_exhausted"
            | "credential_rejected"
            | "provider_overloaded"
            | "provider_server_error"
            | "request_rate_limited"
            | "model_or_overage_limited"
            | "request_rejected"
            | "unattributed_limit"
            | "malformed_evidence"
            | "connection_failure"
            | "timeout" => Some(("outcome", cause)),
            _ => None,
        },
    }
}

#[cfg(any(debug_assertions, test))]
fn write_to(writer: &mut impl std::io::Write, event: ClaudeRouteDiagnostic) {
    if !cfg!(debug_assertions) {
        return;
    }
    if let Some((stage, cause)) = fields(event) {
        // A closed diagnostic is best effort; a broken/full sink does not fail the request.
        let _result = writeln!(
            writer,
            "codex-router claude route diagnostic: stage={stage} cause={cause}"
        );
    }
}

#[cfg(test)]
mod tests {
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
}
