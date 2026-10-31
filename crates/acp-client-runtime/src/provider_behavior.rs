//! Consumer-owned, synchronous provider interpretation and preparation contract.

use session_event_model::{PromptContent, ProviderSubmission, SkillView, ToolCallStatus};

pub trait ProviderBehavior: Send + Sync {
    fn can_reuse_setting_confirmation(&self, kind: crate::ProviderSettingKind) -> bool;
    fn model_metadata_source(&self) -> crate::ModelMetadataSource;
    fn decode_model_metadata(&self, value: &serde_json::Value) -> crate::ModelReasoningMetadata;
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError>;
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError>;
    fn compact(&self) -> Result<PreparedCompact, ProviderError>;
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
