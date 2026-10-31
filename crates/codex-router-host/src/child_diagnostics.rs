//! Privacy-bounded classification of managed child stderr.

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::Counter;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::ChildStderr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChildDiagnosticClass {
    LoopbackConnectionFailure,
    AuditAppendFailure,
    OauthRefreshRejected,
    ModelCatalogSchemaMismatch,
    RemoteControlFailure,
    #[cfg(debug_assertions)]
    ClaudeRoute {
        stage: &'static str,
        cause: &'static str,
    },
    Unclassified,
}

impl ChildDiagnosticClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::LoopbackConnectionFailure => "loopback_connection_failure",
            Self::AuditAppendFailure => "audit_append_failure",
            Self::OauthRefreshRejected => "oauth_refresh_rejected",
            Self::ModelCatalogSchemaMismatch => "model_catalog_schema_mismatch",
            Self::RemoteControlFailure => "remote_control_failure",
            #[cfg(debug_assertions)]
            Self::ClaudeRoute { .. } => "claude_route",
            Self::Unclassified => "unclassified",
        }
    }
}

pub(crate) fn classify_child_stderr(line: &str) -> ChildDiagnosticClass {
    #[cfg(debug_assertions)]
    if line.starts_with("codex-router claude route diagnostic:") {
        return classify_claude_route_diagnostic(line)
            .unwrap_or(ChildDiagnosticClass::Unclassified);
    }
    let normalized = line.to_ascii_lowercase();
    if normalized.starts_with("codex-router loopback connection failed:") {
        ChildDiagnosticClass::LoopbackConnectionFailure
    } else if normalized.starts_with("audit append failed:") {
        ChildDiagnosticClass::AuditAppendFailure
    } else if normalized.contains("invalid_grant")
        && (normalized.contains("oauth") || normalized.contains("refresh token"))
    {
        ChildDiagnosticClass::OauthRefreshRejected
    } else if normalized.contains("failed to refresh available models")
        && normalized.contains("missing field")
    {
        ChildDiagnosticClass::ModelCatalogSchemaMismatch
    } else if normalized.contains("remote control") && normalized.contains("error") {
        ChildDiagnosticClass::RemoteControlFailure
    } else {
        ChildDiagnosticClass::Unclassified
    }
}

#[cfg(debug_assertions)]
fn classify_claude_route_diagnostic(line: &str) -> Option<ChildDiagnosticClass> {
    let fields = line.strip_prefix("codex-router claude route diagnostic: stage=")?;
    let (input_stage, input_cause) = fields.split_once(" cause=")?;
    // Exact, independent consumer allowlist: never retain peer-controlled text.
    let causes: &[&'static str] = match input_stage {
        "arrival" | "unsupported_route" | "attempt_selected" | "upstream_send" => &["none"],
        "body_buffer_failure" | "upstream_preparation_failure" => &["request_rejected"],
        "local_failure" => &[
            "local_auth_rejected",
            "request_rejected",
            "upstream_failure",
            "credential_unavailable",
            "no_openai_accounts_configured",
            "no_eligible_account",
            "short_quota_exhausted",
            "selection_state_unavailable",
            "selector_state_unavailable",
            "affinity_secret_unavailable",
            "malformed_affinity_key",
            "affinity_owner_missing",
            "affinity_owner_unavailable",
        ],
        "outcome" => &[
            "success",
            "shared_window_exhausted",
            "credential_rejected",
            "provider_overloaded",
            "provider_server_error",
            "request_rate_limited",
            "model_or_overage_limited",
            "request_rejected",
            "unattributed_limit",
            "malformed_evidence",
            "connection_failure",
            "timeout",
        ],
        _ => return None,
    };
    let cause = causes.iter().copied().find(|cause| *cause == input_cause)?;
    let stage = [
        "arrival",
        "unsupported_route",
        "attempt_selected",
        "upstream_send",
        "body_buffer_failure",
        "upstream_preparation_failure",
        "local_failure",
        "outcome",
    ]
    .into_iter()
    .find(|stage| *stage == input_stage)?;
    Some(ChildDiagnosticClass::ClaudeRoute { stage, cause })
}

fn classify_managed_child_stderr(source: &str, line: &str) -> ChildDiagnosticClass {
    let diagnostic = classify_child_stderr(line);
    #[cfg(debug_assertions)]
    if matches!(diagnostic, ChildDiagnosticClass::ClaudeRoute { .. }) && source != "router" {
        return ChildDiagnosticClass::Unclassified;
    }
    #[cfg(not(debug_assertions))]
    let _unused = source;
    diagnostic
}

pub(crate) fn spawn_child_stderr_reader(source: &'static str, stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let diagnostic_class = classify_managed_child_stderr(source, &line);
            match diagnostic_class {
                #[cfg(debug_assertions)]
                ChildDiagnosticClass::ClaudeRoute { stage, cause } => tracing::warn!(
                    event.name = "codex_router.host.child_diagnostic",
                    child.source = source,
                    error.kind = diagnostic_class.as_str(),
                    route.stage = stage,
                    route.cause = cause,
                    "managed child emitted Claude route diagnostic"
                ),
                _ => tracing::warn!(
                    event.name = "codex_router.host.child_diagnostic",
                    child.source = source,
                    error.kind = diagnostic_class.as_str(),
                    "managed child emitted stderr"
                ),
            }
            diagnostic_counter().add(
                1,
                &[
                    KeyValue::new("child.source", source),
                    KeyValue::new("error.kind", diagnostic_class.as_str()),
                ],
            );
        }
    });
}

fn diagnostic_counter() -> &'static Counter<u64> {
    static COUNTER: OnceLock<Counter<u64>> = OnceLock::new();
    COUNTER.get_or_init(|| {
        opentelemetry::global::meter("codex-router-host")
            .u64_counter("codex_router_host_child_diagnostic_total")
            .with_description("Count of scrubbed managed-child stderr diagnostics")
            .build()
    })
}

#[cfg(test)]
mod tests {
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
            "codex-router claude route diagnostic: stage=arrival cause=credential_unavailable"
                .into(),
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
            classify_child_stderr(
                "OAuth refresh token was rejected: invalid_grant: Grant not found"
            ),
            ChildDiagnosticClass::OauthRefreshRejected
        );
        assert_eq!(
            classify_child_stderr(
                "failed to refresh available models: missing field `display_name`"
            ),
            ChildDiagnosticClass::ModelCatalogSchemaMismatch
        );
        assert_eq!(
            classify_child_stderr("a diagnostic with SECRET_CANARY payload"),
            ChildDiagnosticClass::Unclassified
        );
    }
}
