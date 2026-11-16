//! Consumer-owned, synchronous provider interpretation and preparation contract.

use session_event_model::{PromptContent, ProviderSubmission, SkillView, ToolCallStatus};

pub trait ProviderBehavior: Send + Sync {
    fn initialization_authentication(&self) -> ProviderInitializationAuthentication;
    fn can_reuse_setting_confirmation(&self, kind: crate::ProviderSettingKind) -> bool;
    fn model_metadata_source(&self) -> crate::ModelMetadataSource;
    fn decode_model_metadata(&self, value: &serde_json::Value) -> crate::ModelReasoningMetadata;
    /// What `auto` means for this agent: its own mode realizing it, its ask-first
    /// fallback, and how actions outside its grant are treated (R3, R7).
    fn auto_permission_mode(&self) -> crate::AutoPermissionMode;
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError>;
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError>;
    fn compact(&self) -> Result<PreparedCompact, ProviderError>;
    /// What a permission request touches when the agent states it only in the
    /// request's title, not in ACP `locations`, `diff` content or
    /// `rawInput` (D1 R4, R6). `None` when the title cannot be read for
    /// certain, so the request asks; the default reads no titles.
    fn stated_permission_target(
        &self,
        _action: PermissionRequestAction,
        _title: &str,
    ) -> Option<StatedPermissionTarget> {
        None
    }
}

/// The class of action a permission request asks for, as the in-folder
/// allowance tells them apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionRequestAction {
    /// A file read, edit, delete or move.
    FileAccess,
    /// One shell command line.
    ShellCommand,
}

/// The target an agent's title states for a permission request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatedPermissionTarget {
    /// The file the action touches.
    File(std::path::PathBuf),
    /// The exact command line the action runs.
    Command(String),
}

/// Connection startup policy; the runtime owns advertised-method validation and ACP I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderInitializationAuthentication {
    NotRequested,
    RequiredAgentMethod { method_id: &'static str },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderTurnInput {
    Prompt(ProviderSubmission),
    Compact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderDispatchRefusal {
    Behavior(ProviderError),
    Busy,
    SettingsUnresolved,
    UnsupportedContent(&'static str),
}

#[derive(Clone, Debug)]
pub enum ProviderUpdate {
    SessionAdmitted,
    Commands {
        commands: Vec<ProviderCommand>,
        metadata: Option<serde_json::Value>,
    },
    ToolStatus {
        tool_id: String,
        status: ToolCallStatus,
        metadata: Option<serde_json::Value>,
    },
    SessionClosed,
}

#[derive(Clone, Debug)]
pub struct ProviderCommand {
    pub invocation_name: String,
    pub description: String,
    pub input_hint: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderFacts {
    pub skill_view: Option<SkillView>,
    pub compaction: Option<ProviderCompactionHint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionTrigger {
    Manual,
    Automatic,
    Unlabelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCompactionHint {
    pub tool_id: String,
    pub status: ToolCallStatus,
    pub trigger: CompactionTrigger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedCompact {
    Prompt(Vec<PromptContent>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderError {
    #[error("invalid or stale skill selection")]
    InvalidSelection,
    #[error("provider does not support this operation or input form")]
    Unsupported,
    #[error("provider skill source is unavailable")]
    SourceUnavailable,
    #[error("provider metadata is malformed")]
    MalformedMetadata,
}

#[cfg(test)]
#[path = "provider_behavior_test.rs"]
mod tests;
