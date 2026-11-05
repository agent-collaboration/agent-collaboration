use super::*;

#[test]
#[cfg(debug_assertions)]
fn claude_route_diagnostic_wire_is_recognized_without_accepting_extra_text() {
    let literal = "codex-router claude route diagnostic: stage=arrival cause=none";
    assert_eq!(classify_child_stderr(literal).as_str(), "claude_route");
    for invalid in [
        format!("{literal} token=SECRET_CANARY"),
        format!("{literal}\nSECRET_CANARY"),
        "codex-router claude route diagnostic: stage=unknown cause=none".into(),
        "codex-router claude route diagnostic: stage=arrival cause=credential_unavailable".into(),
        "codex-router claude route diagnostic: stage=local_failure cause=SECRET_CANARY".into(),
        "codex-router claude route diagnostic: invalid_grant oauth SECRET_CANARY".into(),
    ] {
        assert_eq!(
            classify_child_stderr(&invalid),
            ChildDiagnosticClass::Unclassified
        );
    }
}

#[test]
#[cfg(debug_assertions)]
fn claude_route_fields_keep_only_exact_closed_stage_and_cause() {
    for (stage, cause) in [
        ("arrival", "none"),
        ("unsupported_route", "none"),
        ("attempt_selected", "none"),
        ("upstream_send", "none"),
        ("body_buffer_failure", "request_rejected"),
        ("upstream_preparation_failure", "request_rejected"),
        ("local_failure", "credential_unavailable"),
        ("local_failure", "no_eligible_account"),
        ("outcome", "connection_failure"),
        ("outcome", "provider_server_error"),
        ("outcome", "success"),
    ] {
        let line = format!("codex-router claude route diagnostic: stage={stage} cause={cause}");
        assert_eq!(
            classify_child_stderr(&line),
            ChildDiagnosticClass::ClaudeRoute { stage, cause }
        );
        assert_eq!(
            classify_child_stderr(&format!("{line} SECRET_CANARY")),
            ChildDiagnosticClass::Unclassified
        );
    }
    assert_eq!(
        classify_child_stderr(
            "codex-router claude route diagnostic: stage=outcome cause=not_attempted"
        ),
        ChildDiagnosticClass::Unclassified
    );
}

#[test]
fn only_the_router_child_can_qualify_a_claude_route_line() {
    let line = "codex-router claude route diagnostic: stage=arrival cause=none";
    assert_eq!(
        classify_managed_child_stderr("router", line).as_str(),
        if cfg!(debug_assertions) {
            "claude_route"
        } else {
            "unclassified"
        }
    );
    for source in ["app_server", "provider", "unknown"] {
        assert_eq!(
            classify_managed_child_stderr(source, line),
            ChildDiagnosticClass::Unclassified
        );
    }
}

#[test]
fn app_server_diagnostics_classify_known_failures_without_retaining_raw_lines() {
    assert_eq!(
        classify_child_stderr(
            "codex-router loopback connection failed: severity=debug class=client_disconnect reason=hyper_closed"
        ),
        ChildDiagnosticClass::LoopbackConnectionFailure
    );
    assert_eq!(
        classify_child_stderr("audit append failed: private cause"),
        ChildDiagnosticClass::AuditAppendFailure
    );
    assert_eq!(
        classify_child_stderr("OAuth refresh token was rejected: invalid_grant: Grant not found"),
        ChildDiagnosticClass::OauthRefreshRejected
    );
    assert_eq!(
        classify_child_stderr("failed to refresh available models: missing field `display_name`"),
        ChildDiagnosticClass::ModelCatalogSchemaMismatch
    );
    assert_eq!(
        classify_child_stderr("a diagnostic with SECRET_CANARY payload"),
        ChildDiagnosticClass::Unclassified
    );
}
