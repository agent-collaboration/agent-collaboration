//! Provider-session commands shared by client-facing protocol adapters.
//!
//! The Host supplies the implementation. This crate owns the interface so no
//! front door needs to know the provider runtime or ACP SDK.
use message_board::{Identity, SessionEndpointRef, SessionRef};
use std::{future::Future, path::PathBuf, pin::Pin};

pub type CommandFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, CommandFailure>> + Send + 'a>>;

/// The face and Host share the validated provider-neutral content vocabulary.
pub use session_event_model::PromptContent as CommandContent;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionSettingsCommand {
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionCommand {
    pub endpoint: SessionEndpointRef,
    pub working_directory: PathBuf,
    pub settings: SessionSettingsCommand,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptSessionCommand {
    pub target: SessionRef,
    pub input_id: session_event_model::InputId,
    pub submission: session_event_model::ProviderSubmission,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteerSessionCommand {
    pub target: SessionRef,
    pub input_id: session_event_model::InputId,
    pub expected_turn_id: String,
    pub submission: session_event_model::ProviderSubmission,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueInputCommand {
    pub target: SessionRef,
    pub submission: session_event_model::ProviderSubmission,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSessionInput {
    pub input_id: String,
    pub position: u64,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTurnHandle {
    pub turn_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionSteerOutcome {
    Injected { turn_id: String },
    StartedNewTurn { turn_id: String },
    PromptRequired,
    Failed { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTargetCommand {
    pub target: SessionRef,
    pub actor: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetSessionSettingCommand {
    pub target: SessionRef,
    pub setting_id: String,
    pub value: String,
    pub intent: session_event_model::SettingApplicationIntent,
    pub expected_model: Option<String>,
    pub actor: Identity,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CommandFailure {
    #[error("new session settings are invalid")]
    InvalidCreateSetting {
        target: Option<collaboration_protocol::SessionRef>,
        detail: Box<collaboration_protocol::InvalidProviderSetting>,
    },
    #[error("session was created without all requested settings")]
    CreatedWithoutSettings {
        target: Box<collaboration_protocol::SessionRef>,
        applied: Vec<collaboration_protocol::AppliedProviderSetting>,
        failed: Vec<collaboration_protocol::FailedProviderSetting>,
        not_applied: Vec<collaboration_protocol::NotAppliedProviderSetting>,
    },
    #[error("skill selection is invalid or stale")]
    InvalidSelection,
    #[error("session not found")]
    SessionNotFound,
    #[error("operation is unsupported")]
    Unsupported,
    #[error("operation is unsupported: {operation}")]
    UnsupportedOperation { operation: &'static str },
    #[error("session is busy")]
    Busy,
    #[error("session has no active turn")]
    NoActiveTurn,
    #[error("session settings are unresolved")]
    SettingsUnresolved,
    #[error("setting context changed")]
    SettingsContextChanged,
    #[error("content type is unsupported: {0}")]
    UnsupportedContent(&'static str),
    #[error("setting value is invalid")]
    InvalidSetting { advertised: Vec<String> },
    #[error("actor is not authorized")]
    UnauthorizedActor,
    #[error("provider command failed")]
    ProviderFailure,
}

/// Commands have distinct results and all carry the identity of the caller.
/// Read-side history and live events belong to the session event hub.
pub trait SessionCommandPort: Send + Sync {
    fn create(&self, command: CreateSessionCommand) -> CommandFuture<'_, SessionRef>;
    /// Activates a stored Session after resetting the hub's replay epoch.
    fn load_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()>;
    /// Activates a stored Session without historical replay.
    fn resume_session(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()>;
    fn prompt(&self, command: PromptSessionCommand) -> CommandFuture<'_, SessionTurnHandle>;
    fn compact(&self, command: SessionTargetCommand) -> CommandFuture<'_, SessionTurnHandle>;
    fn steer(&self, command: SteerSessionCommand) -> CommandFuture<'_, SessionSteerOutcome>;
    fn queue_add(&self, command: QueueInputCommand) -> CommandFuture<'_, QueuedSessionInput>;
    fn queue_list(
        &self,
        command: SessionTargetCommand,
    ) -> CommandFuture<'_, Vec<QueuedSessionInput>>;
    fn queue_cancel(
        &self,
        command: SessionTargetCommand,
        input_id: String,
    ) -> CommandFuture<'_, ()>;
    fn cancel(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()>;
    fn close(&self, command: SessionTargetCommand) -> CommandFuture<'_, ()>;
    fn set_setting(&self, command: SetSessionSettingCommand) -> CommandFuture<'_, ()>;
}

#[cfg(test)]
#[path = "session_command_port_test.rs"]
mod tests;
