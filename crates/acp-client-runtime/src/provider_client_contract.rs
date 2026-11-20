//! Provider admission, prompt result, and safe ACP error contracts.
use crate::AcpProtocolVersion;
use agent_client_protocol::AcpAgentConfig;
#[cfg(any(test, feature = "test-observation"))]
use agent_client_protocol::schema::v1::ToolKind;
use session_event_model::StopReason as ProviderPromptStopReason;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

/// The largest prompt output a turn keeps; the rest is cut off.
pub(crate) const MAX_PROMPT_OUTPUT_BYTES: usize = 1024 * 1024;

/// What a prompt's dispatch observer hears: it was submitted, it wasn't,
/// or a session refusal stopped it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderPromptDispatchObservation {
    Submitted,
    NotSubmitted,
    Refused(crate::ProviderDispatchRefusal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderErrorCorrelationId(pub(crate) uuid::Uuid);

impl ProviderErrorCorrelationId {
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Display for ProviderErrorCorrelationId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0.hyphenated())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderLaunch {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub persistence_target: crate::ProviderPersistenceTarget,
}

impl ExternalProviderLaunch {
    pub(crate) fn sdk_config(&self) -> AcpAgentConfig {
        AcpAgentConfig::new(&self.executable)
            .args(self.arguments.clone())
            .envs(self.environment.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderAdmission {
    pub runtime_name: Option<String>,
    pub runtime_version: Option<String>,
    pub supports_load: bool,
    pub supports_mcp_http: bool,
    pub supports_steering: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderPromptOutcome {
    pub output: String,
    pub stop_reason: ProviderPromptStopReason,
    pub permission_refusal_reason: Option<ExternalProviderApprovalRefusalReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProviderApprovalRefusalReason {
    MissingPromptContext,
    ApprovalBrokerUnavailable,
    Managed(session_event_model::managed_interaction_refusal::ManagedInteractionRefusal),
}

impl ExternalProviderApprovalRefusalReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingPromptContext => "missingPromptContext",
            Self::ApprovalBrokerUnavailable => "approvalBrokerUnavailable",
            Self::Managed(refusal) => refusal.code(),
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::Managed(refusal) => refusal.reason(),
            reason => reason.code(),
        }
    }
}

#[cfg(any(test, feature = "test-observation"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderApprovalRefusalWarning {
    pub endpoint: String,
    pub provider_session_id: String,
    pub method: &'static str,
    pub reason_code: ExternalProviderApprovalRefusalReason,
}

#[cfg(any(test, feature = "test-observation"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderToolCall {
    pub name: Option<String>,
    pub title: String,
    pub kind: ToolKind,
    pub status: agent_client_protocol::schema::v1::ToolCallStatus,
    pub outcome: ExternalProviderToolOutcome,
}

#[cfg(any(test, feature = "test-observation"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProviderToolOutcome {
    Success,
    Error,
    Rejected,
    PermissionDenied,
    Unknown,
}

#[cfg(any(test, feature = "test-observation"))]
pub(crate) fn classify_mcp_tool_outcome(
    raw_output: Option<&serde_json::Value>,
) -> ExternalProviderToolOutcome {
    let Some(output) = raw_output.and_then(serde_json::Value::as_object) else {
        return ExternalProviderToolOutcome::Unknown;
    };
    if output.get("error").is_some() {
        ExternalProviderToolOutcome::Error
    } else if output.get("rejected") == Some(&serde_json::Value::Bool(true)) {
        ExternalProviderToolOutcome::Rejected
    } else if output.get("permissionDenied") == Some(&serde_json::Value::Bool(true)) {
        ExternalProviderToolOutcome::PermissionDenied
    } else if output.get("success") == Some(&serde_json::Value::Bool(true)) {
        ExternalProviderToolOutcome::Success
    } else {
        ExternalProviderToolOutcome::Unknown
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderCreatedSession {
    pub provider_session_id: String,
    pub effective_settings: crate::EffectiveProviderSettings,
    /// The requested and actual permission mode, read back from the agent (R3).
    pub permission_mode: crate::PermissionModeReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSessionSummary {
    pub provider_session_id: String,
    pub cwd: PathBuf,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

#[cfg(any(test, feature = "test-observation"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProviderPermissionOutcome {
    Cancelled,
    Selected,
}

#[cfg(any(test, feature = "test-observation"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalProviderPermissionObservation {
    pub method: &'static str,
    pub request_count: u64,
    pub last_outcome: Option<ExternalProviderPermissionOutcome>,
    pub execute_tool_call_count: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ExternalProviderRuntimeError {
    #[error(transparent)]
    Behavior(#[from] crate::ProviderError),
    #[error(
        "explicit compaction was not confirmed by a completed manual marker and successful terminal"
    )]
    CompactionUnconfirmed,
    #[error("provider process could not be launched: {0}")]
    Launch(String),
    #[error("provider ACP initialization failed: {0}")]
    Initialize(String),
    #[error("provider ACP initialization timed out")]
    InitializeTimeout,
    #[error("provider selected unsupported ACP protocol version {actual:?}")]
    UnsupportedProtocol { actual: AcpProtocolVersion },
    /// The session is busy; `answer` names its state and the valid next
    /// steps (D1 M8).
    #[error("{answer}")]
    LocalBusy { answer: String },
    /// A setting holds the session; `answer` names its state and the valid
    /// next steps (D1 M8).
    #[error("{answer}")]
    SettingsUnresolved { answer: String },
    #[error("confirmed model changed before applying the dependent setting")]
    SettingsContextChanged,
    #[error(
        "settings-uncertain: run conversation settings set to resolve the unknown provider setting outcome"
    )]
    SettingsOutcomeUncertain,
    #[error("fast is not offered for the current model")]
    FastNotOffered,
    #[error("provider setting could not be applied")]
    SettingFailed {
        setting: provider_session_machines::ProviderSettingKind,
        value: String,
        reason: String,
    },
    #[error("outcomeUnknown: provider setting effect is not confirmed")]
    SettingOutcomeUnknown {
        provider_session_id: String,
        setting: provider_session_machines::ProviderSettingKind,
        value: String,
    },
    #[error("provider does not advertise steering")]
    UnsupportedSteering,
    #[error("provider does not advertise {capability}")]
    UnsupportedCapability { capability: &'static str },
    #[error("provider conversation operation is not active")]
    LocalNotFound,
    /// The Router released the session (closed it here for an agent with no
    /// session/close), so it isn't loaded or resumed again on this
    /// connection, even by a load that was already in flight (D1 R14).
    #[error("provider session was closed by the Router")]
    SessionReleased,
    /// The session's state refused the request; `answer` names the state and
    /// the valid next steps (D1 M8).
    #[error("{answer}")]
    SessionStateRefused { answer: String },
    #[error("provider cancellation target is no longer active")]
    LocalCancelTargetMismatch,
    #[error("provider authentication is required (ACP code {code})")]
    AuthenticationRequired {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error(
        "the provider no longer knows this session, so start a new session with the same setup (ACP code {code})"
    )]
    ProviderSessionNotFound {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("the provider did not answer the restore in time, so retry this session")]
    RestoreNoAnswer,
    #[error(
        "the provider disconnected before confirming the restore, so reconnect and retry this session"
    )]
    RestoreDisconnected,
    #[error("the provider failed while restoring this session, so retry it (ACP code {code})")]
    RestoreFailed {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error(
        "the provider refused to restore this session, so check the provider before retrying (ACP code {code})"
    )]
    RestoreRefused {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider resource was not found (ACP code {code})")]
    ResourceNotFound {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider ACP method is unsupported (ACP code {code})")]
    UnsupportedMethod {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider ACP parameters are invalid (ACP code {code})")]
    InvalidParams {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider ACP request was cancelled (ACP code {code})")]
    RequestCancelled {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider rejected the ACP operation (ACP code {code})")]
    ProviderRejected {
        code: i64,
        correlation_id: ProviderErrorCorrelationId,
    },
    #[error("provider operation response was unavailable")]
    TransportFailure,
    #[error("session event consumer closed")]
    SinkClosed,
    #[error("invalidSetting: value requires an unambiguous advertised provider setting option")]
    InvalidSetting {
        setting: provider_session_machines::ProviderSettingKind,
        value: String,
        advertised: Vec<String>,
        provider_session_id: String,
        disposition: crate::InvalidSettingSessionDisposition,
    },
    #[error("createdWithoutSettings: provider session needs setting resolution")]
    CreatedWithoutSettings {
        provider_session_id: String,
        applied: Box<[crate::AppliedProviderSetting]>,
        failed: crate::FailedProviderSetting,
        not_applied: Box<[crate::NotAppliedProviderSetting]>,
        /// The requested and confirmed mode, finished before the failed setting.
        /// Boxed so the error stays small on every Result path.
        permission_mode: Box<crate::PermissionModeReport>,
    },
    #[error("session history replay admission failed before provider load")]
    HistoryReplayAdmissionFailed,
    #[error("session history replay could not begin")]
    HistoryReplayUnavailable,
    #[error(
        "provider prompt output exceeded the retained output limit; cancellation was requested and settled"
    )]
    PromptOutputLimitExceeded,
    #[error("provider ACP frame exceeded the configured transport limit")]
    FrameLimitExceeded,
    #[error("provider ACP message could not be decoded or classified")]
    FrameDecodeFailure,
    #[error("unsupportedContent{{{content_type}}}")]
    UnsupportedContent { content_type: &'static str },
    #[error("provider ACP operation failed: {0}")]
    Operation(String),
}

pub(crate) fn sanitized_initialization_error(error: &agent_client_protocol::Error) -> String {
    sanitized_acp_error(error, "initialize", "initialize")
}

pub(crate) fn sanitized_acp_error(
    error: &agent_client_protocol::Error,
    method: &str,
    stage: &str,
) -> String {
    let error_data_bytes = error.data.as_ref().map_or(0, |data| data.to_string().len());
    format!(
        "ACP request failed (method={method}, stage={stage}, code={}, error_data_bytes={error_data_bytes})",
        i32::from(error.code),
    )
}

pub(crate) fn provider_frame_decode_error(
    error: agent_client_protocol::Error,
) -> ExternalProviderRuntimeError {
    if error.to_string().contains("max line length exceeded") {
        ExternalProviderRuntimeError::FrameLimitExceeded
    } else {
        ExternalProviderRuntimeError::FrameDecodeFailure
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderSteeringOutcome<OperationId, Context> {
    Injected {
        running_operation_id: Option<OperationId>,
        /// The approval context of the turn the input joined, captured when
        /// the steer was sent (design note §12.4): the caller reads the
        /// joined turn's approver from it, not from a shared record.
        joined_turn: Option<Context>,
    },
    PromptRequired,
    StartedNewTurn,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSessionActivity {
    NotLoaded,
    Idle,
    Running,
}
