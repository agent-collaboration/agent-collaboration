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
#[path = "claude_route_diagnostics_test.rs"]
mod tests;
