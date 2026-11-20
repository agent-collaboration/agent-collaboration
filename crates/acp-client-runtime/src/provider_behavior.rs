//! Consumer-owned, synchronous provider interpretation and preparation contract.

use session_event_model::{PromptContent, ProviderSubmission, SkillView, ToolCallStatus};
use std::path::PathBuf;

/// The folders a Router-started session in auto works in without asking,
/// beyond its working folder (D1 R18): its scratch folder and the Router's
/// work roots that exist, and for Claude the command its hook runs to make
/// it ask outside them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionWorkRoots {
    pub roots: Vec<PathBuf>,
    pub ask_outside_command: Option<String>,
    /// The session starts in Router auto, so its agent must not run without
    /// the boundary these roots define.
    pub router_auto: bool,
}

/// What a load or resume rebuilds of the setup a session was created with
/// (D1 R18; owner Decision 3): the mode its create asked for, its scratch
/// folder and its work roots, built as a create builds them. The default
/// rebuilds nothing, as for a session recorded before these were kept: no
/// additional folders, no Router allowance, every request asks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionRestoreSetup {
    pub requested_mode: Option<String>,
    pub scratch_folder: Option<PathBuf>,
    pub work_roots: SessionWorkRoots,
}

pub trait ProviderBehavior: Send + Sync {
    fn initialization_authentication(&self) -> ProviderInitializationAuthentication;
    fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool;
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

    /// The `_meta` a new session's request carries so the agent's own
    /// settings take the Router's work roots (D1 R18); the default sends
    /// none.
    fn session_new_meta(
        &self,
        _work_roots: &SessionWorkRoots,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        None
    }

    /// Whether the adapter recognizes a missing session in an InvalidParams
    /// reply to load or resume for this exact ID.
    fn restore_invalid_params_means_session_not_found(
        &self,
        _requested_session_id: &str,
        _data: &serde_json::Value,
    ) -> bool {
        false
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

/// Why a prompt wasn't dispatched. Each session refusal carries its one-line
/// answer: the state and the valid next steps (D1 M8).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderDispatchRefusal {
    Behavior(ProviderError),
    Busy {
        answer: String,
    },
    SettingsUnresolved {
        answer: String,
    },
    /// The session isn't live on this connection: not loaded, or loading.
    NotLoaded {
        answer: String,
    },
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
