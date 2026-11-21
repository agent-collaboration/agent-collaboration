//! Safe ACP error-code classification at the provider client boundary.

use crate::provider_client_contract::{ExternalProviderRuntimeError, ProviderErrorCorrelationId};
use agent_client_protocol::schema::v1::ErrorCode;
use provider_session_machines::session_owner::{ProviderWireFailure, ProviderWireFailureKind};

impl ExternalProviderRuntimeError {
    #[must_use]
    pub fn coded_wire_failure(&self) -> Option<ProviderWireFailure> {
        use ExternalProviderRuntimeError as RuntimeError;
        use ProviderWireFailureKind as Kind;

        let (kind, code, correlation_id) = match *self {
            RuntimeError::AuthenticationRequired {
                code,
                correlation_id,
            } => (Kind::AuthenticationRequired, code, correlation_id),
            RuntimeError::ProviderSessionNotFound {
                code,
                correlation_id,
            } => (Kind::SessionNotFound, code, correlation_id),
            RuntimeError::ResourceNotFound {
                code,
                correlation_id,
            } => (Kind::ResourceNotFound, code, correlation_id),
            RuntimeError::UnsupportedMethod {
                code,
                correlation_id,
            } => (Kind::UnsupportedMethod, code, correlation_id),
            RuntimeError::InvalidParams {
                code,
                correlation_id,
            } => (Kind::InvalidParams, code, correlation_id),
            RuntimeError::RequestCancelled {
                code,
                correlation_id,
            } => (Kind::RequestCancelled, code, correlation_id),
            RuntimeError::ProviderRejected {
                code,
                correlation_id,
            } => (Kind::Rejected, code, correlation_id),
            RuntimeError::RestoreFailed {
                code,
                correlation_id,
            } => (Kind::RestoreFailed, code, correlation_id),
            RuntimeError::RestoreRefused {
                code,
                correlation_id,
            } => (Kind::RestoreRefused, code, correlation_id),
            _ => return None,
        };
        Some(ProviderWireFailure {
            kind,
            code,
            correlation_id: correlation_id.0,
        })
    }
}

impl From<ProviderWireFailure> for ExternalProviderRuntimeError {
    fn from(failure: ProviderWireFailure) -> Self {
        use ProviderWireFailureKind as Kind;

        let code = failure.code;
        let correlation_id = ProviderErrorCorrelationId(failure.correlation_id);
        match failure.kind {
            Kind::AuthenticationRequired => Self::AuthenticationRequired {
                code,
                correlation_id,
            },
            Kind::SessionNotFound => Self::ProviderSessionNotFound {
                code,
                correlation_id,
            },
            Kind::ResourceNotFound => Self::ResourceNotFound {
                code,
                correlation_id,
            },
            Kind::UnsupportedMethod => Self::UnsupportedMethod {
                code,
                correlation_id,
            },
            Kind::InvalidParams => Self::InvalidParams {
                code,
                correlation_id,
            },
            Kind::RequestCancelled => Self::RequestCancelled {
                code,
                correlation_id,
            },
            Kind::Rejected => Self::ProviderRejected {
                code,
                correlation_id,
            },
            Kind::RestoreFailed => Self::RestoreFailed {
                code,
                correlation_id,
            },
            Kind::RestoreRefused => Self::RestoreRefused {
                code,
                correlation_id,
            },
        }
    }
}

// Only the adapter's finite reported convention is retained. It is not vendor attribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReportedProviderErrorKind {
    AuthenticationFailed,
    OauthOrgNotAllowed,
    AccountOnHold,
    VerificationRequired,
    BillingError,
    RateLimit,
    Overloaded,
    InvalidRequest,
    ModelNotFound,
    ServerError,
    SdkUnknown,
    MaxOutputTokens,
    CloudCredentialError,
    WorkerShutdown,
    TransportLost,
    NoResult,
    Unclassified,
}

impl ReportedProviderErrorKind {
    fn from_error_data(data: Option<&serde_json::Value>) -> Self {
        match data
            .and_then(|value| value.as_object())
            .and_then(|object| object.get("errorKind"))
            .and_then(|value| value.as_str())
        {
            Some("authentication_failed") => Self::AuthenticationFailed,
            Some("oauth_org_not_allowed") => Self::OauthOrgNotAllowed,
            Some("account_on_hold") => Self::AccountOnHold,
            Some("verification_required") => Self::VerificationRequired,
            Some("billing_error") => Self::BillingError,
            Some("rate_limit") => Self::RateLimit,
            Some("overloaded") => Self::Overloaded,
            Some("invalid_request") => Self::InvalidRequest,
            Some("model_not_found") => Self::ModelNotFound,
            Some("server_error") => Self::ServerError,
            Some("unknown") => Self::SdkUnknown,
            Some("max_output_tokens") => Self::MaxOutputTokens,
            Some("cloud_credential_error") => Self::CloudCredentialError,
            Some("worker_shutdown") => Self::WorkerShutdown,
            Some("transport_lost") => Self::TransportLost,
            Some("no_result") => Self::NoResult,
            _ => Self::Unclassified,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "authentication_failed",
            Self::OauthOrgNotAllowed => "oauth_org_not_allowed",
            Self::AccountOnHold => "account_on_hold",
            Self::VerificationRequired => "verification_required",
            Self::BillingError => "billing_error",
            Self::RateLimit => "rate_limit",
            Self::Overloaded => "overloaded",
            Self::InvalidRequest => "invalid_request",
            Self::ModelNotFound => "model_not_found",
            Self::ServerError => "server_error",
            Self::SdkUnknown => "unknown",
            Self::MaxOutputTokens => "max_output_tokens",
            Self::CloudCredentialError => "cloud_credential_error",
            Self::WorkerShutdown => "worker_shutdown",
            Self::TransportLost => "transport_lost",
            Self::NoResult => "no_result",
            Self::Unclassified => "Unclassified",
        }
    }
}

pub(crate) fn acp_operation_error(
    error: agent_client_protocol::Error,
) -> ExternalProviderRuntimeError {
    if agent_client_protocol::is_incoming_transport_closed(&error) {
        return ExternalProviderRuntimeError::TransportFailure;
    }
    let reported_error_kind = ReportedProviderErrorKind::from_error_data(error.data.as_ref());
    let code = i64::from(i32::from(error.code));
    match error.code {
        ErrorCode::AuthRequired => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::AuthenticationRequired {
                code,
                correlation_id,
            }
        }
        ErrorCode::ResourceNotFound => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::ResourceNotFound {
                code,
                correlation_id,
            }
        }
        ErrorCode::MethodNotFound => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::UnsupportedMethod {
                code,
                correlation_id,
            }
        }
        ErrorCode::InvalidParams => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::InvalidParams {
                code,
                correlation_id,
            }
        }
        ErrorCode::RequestCancelled => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::RequestCancelled {
                code,
                correlation_id,
            }
        }
        _ => {
            let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
            ExternalProviderRuntimeError::ProviderRejected {
                code,
                correlation_id,
            }
        }
    }
}

pub(crate) fn acp_load_session_error(
    error: agent_client_protocol::Error,
    requested_session_id: &str,
    behavior: &dyn crate::ProviderBehavior,
) -> ExternalProviderRuntimeError {
    if agent_client_protocol::is_incoming_transport_closed(&error) {
        return ExternalProviderRuntimeError::RestoreDisconnected;
    }
    if error.code == ErrorCode::ResourceNotFound
        || (error.code == ErrorCode::InvalidParams
            && error.data.as_ref().is_some_and(|data| {
                behavior.restore_invalid_params_means_session_not_found(requested_session_id, data)
            }))
    {
        let reported_error_kind = ReportedProviderErrorKind::from_error_data(error.data.as_ref());
        let code = i64::from(i32::from(error.code));
        let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
        return ExternalProviderRuntimeError::ProviderSessionNotFound {
            code,
            correlation_id,
        };
    }
    if error.code == ErrorCode::InternalError {
        let reported_error_kind = ReportedProviderErrorKind::from_error_data(error.data.as_ref());
        let code = i64::from(i32::from(error.code));
        let correlation_id = log_provider_error(code, &error.message, reported_error_kind);
        return ExternalProviderRuntimeError::RestoreFailed {
            code,
            correlation_id,
        };
    }
    match acp_operation_error(error) {
        ExternalProviderRuntimeError::ProviderRejected {
            code,
            correlation_id,
        } => ExternalProviderRuntimeError::RestoreRefused {
            code,
            correlation_id,
        },
        failure => failure,
    }
}

fn log_provider_error(
    code: i64,
    raw_provider_text: &str,
    reported_error_kind: ReportedProviderErrorKind,
) -> ProviderErrorCorrelationId {
    let correlation_id = ProviderErrorCorrelationId::generate();
    record_reported_error_category(code, &correlation_id, reported_error_kind);
    tracing::warn!(
        provider_code = code,
        correlation_id = %correlation_id,
        raw_provider_text,
        "provider returned an ACP error"
    );
    correlation_id
}

// Match the existing opt-in debug sink; the legacy raw warning is not broadened.
fn record_reported_error_category(
    code: i64,
    correlation_id: &ProviderErrorCorrelationId,
    reported_error_kind: ReportedProviderErrorKind,
) {
    const DIAGNOSTIC_TARGET: &str = "acp_client_runtime::provider_diagnostics";
    if cfg!(debug_assertions) && tracing::enabled!(target: DIAGNOSTIC_TARGET, tracing::Level::DEBUG)
    {
        tracing::debug!(target: DIAGNOSTIC_TARGET, {
            event.name = "codex_router.acp.error_diagnostic",
            provider_code = code,
            correlation_id = %correlation_id,
            reported_error_kind = reported_error_kind.as_str(),
        }, "Observed ACP reported error category");
    }
}
