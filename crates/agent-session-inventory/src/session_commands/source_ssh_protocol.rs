//! The single private wire contract shared by the caller and source Sessions process.
use super::router_connection_registry::{
    AbsoluteRemoteCwd, AbsoluteSourcePath, RouterConnectionProfile, SourceClientProfile,
};
use codex_native_integration::ResumeModelChoice;
use collaboration_client::protocol::{
    CodexGeneration, ConversationOperationFailure, EndpointInventory, EndpointRef,
    NativeSessionListParams, NativeSessionListResult, OperationId, ProviderBindingIdentity,
    ProviderSessionListParams, ProviderSessionListResult, SchemaDigest, SessionRef, UuidIdentity,
};
use serde::{Deserialize, Serialize};

pub const SOURCE_REQUEST_SWITCH: &str = "--source-request-v1";
pub const MAX_SOURCE_REQUEST_BYTES: usize = 65536;
pub const MAX_SOURCE_REPLY_BYTES: usize =
    4 * collaboration_client::protocol::MAX_CONTROL_FRAME_BYTES;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRequest {
    pub service_directory: AbsoluteSourcePath,
    pub expected_service: UuidIdentity,
    pub operation: SourceOperation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceOperation {
    Endpoints,
    ClientInventory,
    ProviderInspect {
        binding: ProviderBindingIdentity,
        target: SessionRef,
    },
    NativeSessions {
        params: NativeSessionListParams,
    },
    ProviderSessions {
        params: ProviderSessionListParams,
    },
    ProviderOperationShow {
        service_epoch: UuidIdentity,
        operation_id: OperationId,
    },
    Prepare {
        action: SourceSessionAction,
        client_profile: SourceClientProfile,
    },
    NativeClaude {
        service_epoch: UuidIdentity,
        action: SourceNativeClaudeAction,
    },
    ProviderAction {
        operation_id: OperationId,
        binding: ProviderBindingIdentity,
        action: SourceManagedAction,
    },
    Launch {
        prepared: Box<PreparedSourceLaunch>,
        options: SourceNativeOptions,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceReply {
    ProviderInspection {
        binding: Box<ProviderBindingIdentity>,
        inspection: Box<collaboration_client::protocol::ProviderSessionInspectResult>,
    },
    #[serde(rename_all = "camelCase")]
    ClientInventory {
        inventory: EndpointInventory,
        native_claude: SourceNativeLaunchAvailability,
    },
    Endpoints {
        inventory: EndpointInventory,
    },
    NativeSessions {
        page: NativeSessionListResult,
    },
    ProviderSessions {
        page: ProviderSessionListResult,
    },
    ProviderOperation {
        #[serde(rename = "operationId")]
        operation_id: OperationId,
        result: Box<SourceProviderOperationRead>,
    },
    Prepared {
        launch: Box<PreparedSourceLaunch>,
    },
    ProviderAction {
        result: Box<SourceProviderActionResult>,
    },
    NativeClaude {
        result: Box<SourceNativeClaudeResult>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceNativeLaunchAvailability {
    Ready {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<SourceNativeClaudeCapability>,
    },
    Unavailable {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceNativeClaudeCapability {
    /// Resume refuses running jobs and live peers, and requires complete Loaded reads.
    GuardedResume,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceNativeClaudeAction {
    New {
        directory: AbsoluteRemoteCwd,
        arguments: Vec<std::ffi::OsString>,
    },
    Resume {
        target: SessionRef,
        directory: AbsoluteRemoteCwd,
        arguments: Vec<std::ffi::OsString>,
    },
    Fork {
        target: SessionRef,
        directory: AbsoluteRemoteCwd,
        arguments: Vec<std::ffi::OsString>,
    },
}

impl SourceNativeClaudeAction {
    pub fn captured_identity(&self) -> SourceNativeClaudeIdentity {
        match self {
            Self::New { directory, .. } => SourceNativeClaudeIdentity::New {
                directory: directory.clone(),
            },
            Self::Resume {
                target, directory, ..
            } => SourceNativeClaudeIdentity::Resume {
                target: target.clone(),
                directory: directory.clone(),
            },
            Self::Fork {
                target, directory, ..
            } => SourceNativeClaudeIdentity::Fork {
                target: target.clone(),
                directory: directory.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceNativeClaudeIdentity {
    New {
        directory: AbsoluteRemoteCwd,
    },
    Resume {
        target: SessionRef,
        directory: AbsoluteRemoteCwd,
    },
    Fork {
        target: SessionRef,
        directory: AbsoluteRemoteCwd,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceNativeBinding {
    pub service_id: UuidIdentity,
    pub service_epoch: UuidIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceNativeClaudeResult {
    pub source: SourceNativeBinding,
    pub action: SourceNativeClaudeIdentity,
    pub observation: SourceNativeObservation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceNativeExitStatus {
    Exited { code: i32 },
    Terminated { description: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceNativeUnknownReason {
    MissingReceipt,
    OutputUnavailable { message: String },
    TimedOut,
    TransportUnavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceNativeObservation {
    Observed {
        status: SourceNativeExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Unknown {
        reason: SourceNativeUnknownReason,
        status: Option<SourceNativeExitStatus>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceManagedAction {
    Create { directory: AbsoluteRemoteCwd },
    Load { target: SessionRef },
    Resume { target: SessionRef },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceProviderActionResult {
    ManagedCreate {
        result: Box<collaboration_client::protocol::ConversationCreateOutcome>,
    },
    ManagedLoad {
        result: Box<collaboration_client::ConversationOperationResult>,
    },
    ManagedResume {
        result: Box<collaboration_client::protocol::ConversationOperationWaitResult>,
    },
    Failed {
        operation_id: OperationId,
        target: Option<SessionRef>,
        failure: Box<ConversationOperationFailure>,
    },
    Unknown {
        operation_id: OperationId,
        target: Option<SessionRef>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceProviderOperationRead {
    Found {
        operation: Box<collaboration_client::protocol::ConversationOperationSnapshot>,
    },
    Failed {
        failure: Box<ConversationOperationFailure>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceResponse {
    Ready { reply: SourceReply },
    Rejected { reason: SourceRejection },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceSessionAction {
    New {
        directory: AbsoluteRemoteCwd,
    },
    Resume {
        target: SessionRef,
        stored_model: SourceStoredModel,
    },
    Fork {
        target: SessionRef,
        stored_model: SourceStoredModel,
    },
}

impl SourceSessionAction {
    pub fn target(&self) -> Option<&SessionRef> {
        match self {
            Self::New { .. } => None,
            Self::Resume { target, .. } | Self::Fork { target, .. } => Some(target),
        }
    }

    pub fn validate(&self, service: &UuidIdentity) -> Result<(), SourceRejection> {
        match self {
            Self::New { .. } => Ok(()),
            Self::Resume {
                target,
                stored_model,
            }
            | Self::Fork {
                target,
                stored_model,
            } => {
                if target.endpoint.service_id != *service {
                    return Err(SourceRejection::WrongService);
                }
                stored_model.validate()
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceStoredModel {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl SourceStoredModel {
    pub fn from_choice(choice: &ResumeModelChoice) -> Self {
        Self {
            model: choice.stored_model().map(str::to_owned),
            reasoning_effort: choice.stored_reasoning_effort().map(str::to_owned),
        }
    }

    pub fn validate(&self) -> Result<(), SourceRejection> {
        validate_model_values(self.model.as_deref(), self.reasoning_effort.as_deref())
    }

    pub fn native_choice(&self) -> ResumeModelChoice {
        ResumeModelChoice::from_stored_values(
            self.model.as_deref(),
            self.reasoning_effort.as_deref(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedSourceLaunch {
    pub action: SourceSessionAction,
    pub endpoint: EndpointRef,
    pub generation: CodexGeneration,
    pub schema_digest: SchemaDigest,
    pub working_directory: AbsoluteRemoteCwd,
    pub client_profile: SourceClientProfile,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceNativeOptions {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub no_alt_screen: bool,
}

impl SourceNativeOptions {
    pub fn parse(arguments: &[std::ffi::OsString]) -> Result<Self, SourceRejection> {
        super::source_native_options::parse(arguments)
    }

    pub fn validate(&self) -> Result<(), SourceRejection> {
        validate_model_values(self.model.as_deref(), self.reasoning_effort.as_deref())
    }
}

fn validate_model_values(model: Option<&str>, effort: Option<&str>) -> Result<(), SourceRejection> {
    if [model, effort]
        .into_iter()
        .flatten()
        .any(|value| value.trim().is_empty() || value.len() > 1024)
        || ResumeModelChoice::rejects_stored_value(model, effort)
    {
        return Err(SourceRejection::ConflictingArguments);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSourceSession {
    pub profile: RouterConnectionProfile,
    pub launch: PreparedSourceLaunch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum SourceRejection {
    #[error("source request is invalid")]
    InvalidRequest,
    #[error("native Claude {inventory_source} inventory: {reason}")]
    InvalidNativeInventory {
        #[serde(rename = "source")]
        inventory_source: collaboration_client::protocol::NativeInventorySource,
        reason: collaboration_client::protocol::NativeInventoryInvalidReason,
    },
    #[error("source connection is unsupported")]
    UnsupportedConnection,
    #[error("selected Router service does not match")]
    WrongService,
    #[error("selected native endpoint is unavailable")]
    UnavailableEndpoint,
    #[error("selected native connection changed; choose again")]
    StaleBinding,
    #[error("source native channel is invalid")]
    InvalidNativeChannel,
    #[error("source session is unavailable")]
    SessionUnavailable,
    #[error("source working directory is unavailable")]
    DirectoryUnavailable,
    #[error("source profile contains a permission override")]
    ProfilePermissionConflict,
    #[error("configured launch accepts only model, effort and terminal display options")]
    ConflictingArguments,
    #[error("selected provider does not advertise this action")]
    UnsupportedProviderAction,
    #[error("source Router is unavailable")]
    SourceUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SourceReadError {
    #[error("{0}")]
    Rejected(SourceRejection),
    #[error("trusted source SSH read is unavailable")]
    ReadUnavailable,
    #[error("source reply is invalid")]
    InvalidReply,
    #[error("source does not advertise guarded native Claude resume")]
    UnguardedNativeResume,
}
