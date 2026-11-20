//! Decisions and ordered effects for one provider session across its runs.
//! The caller executes effects and returns stamped observations as events.

mod owner_reducer;

use crate::session_cell_machine::{SessionCell, SessionCellRefusal};
use crate::session_lifecycle_machine::{CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle};
use crate::session_setting_machine::{SettingApplication, SettingValueId};
use crate::session_turn_machine::{SessionTurn, TurnId};
use crate::tool_ownership::{ToolObservation, ToolOwnership};
use crate::{ProviderSettingKind, SessionGeneration};
use session_event_model::InputId;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunAttempt<Attempt> {
    pub generation: SessionGeneration,
    pub attempt: Attempt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerInput {
    pub generation: SessionGeneration,
    pub input: InputId,
    pub turn: TurnId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerSetting {
    pub change: RunAttempt<SettingApplication>,
    pub kind: ProviderSettingKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptAdmission {
    Direct,
    Queue,
    Auto { steering_supported: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerCommand {
    Restore {
        attempt: LoadAttempt,
        via: RestoreVia,
    },
    Close {
        generation: SessionGeneration,
        attempt: CloseAttempt,
    },
    Prompt {
        input: OwnerInput,
        admission: PromptAdmission,
    },
    Steer {
        input: OwnerInput,
        supported: bool,
    },
    CancelTurn {
        generation: SessionGeneration,
        turn: TurnId,
    },
    CancelQueued {
        generation: SessionGeneration,
        input: InputId,
    },
    Set {
        setting: OwnerSetting,
        value: SettingValueId,
    },
    AcceptSettings {
        generation: SessionGeneration,
    },
}

/// The local decision can finish while the peer's effect remains unknown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireOutcome<Reply> {
    Confirmed(Reply),
    Refused,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoredMode {
    Observed,
    Reapply(SettingValueId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingObservation {
    Applied(SettingValueId),
    Refused { current: Option<SettingValueId> },
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionWireResult {
    Restore {
        run: RunAttempt<LoadAttempt>,
        outcome: WireOutcome<RestoredMode>,
    },
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        outcome: WireOutcome<()>,
    },
    Close {
        run: RunAttempt<CloseAttempt>,
        outcome: WireOutcome<()>,
    },
    Turn {
        input: OwnerInput,
        outcome: WireOutcome<()>,
    },
    Steer {
        input: OwnerInput,
        outcome: WireOutcome<()>,
    },
    Setting {
        setting: OwnerSetting,
        observation: SettingObservation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectOutcome {
    Applied,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStoreResult {
    RestoreSetup {
        run: RunAttempt<LoadAttempt>,
        outcome: EffectOutcome,
    },
    Dispatch {
        input: OwnerInput,
        outcome: EffectOutcome,
    },
    UndoDispatch {
        input: OwnerInput,
        outcome: EffectOutcome,
    },
    SteerDispatch {
        input: OwnerInput,
        outcome: EffectOutcome,
    },
    ConfirmedSetting {
        setting: OwnerSetting,
        outcome: EffectOutcome,
    },
    DispatchReadback {
        input: OwnerInput,
        outcome: EffectOutcome,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutedSessionToolEvent {
    Observed {
        generation: SessionGeneration,
        tool_id: String,
        observation: ToolObservation,
    },
    RetiredUpdate {
        generation: SessionGeneration,
        tool_id: String,
        item_exists: bool,
    },
    /// The connection has already resolved the callback to this session.
    RoutedCallback {
        generation: SessionGeneration,
        tool_id: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreWait {
    Setup,
    Wire,
    Mode,
    Publication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputWait {
    Admission,
    DispatchStore,
    UndoStore,
    Wire,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingWait {
    Wire,
    Store,
}

/// The executor delivers a firing for the exact wait it armed on the
/// injected clock. The reducer never reads time or invents deadline values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionDeadline {
    Restore {
        run: RunAttempt<LoadAttempt>,
        wait: RestoreWait,
    },
    Close(RunAttempt<CloseAttempt>),
    Prompt {
        input: OwnerInput,
        wait: InputWait,
    },
    Steer {
        input: OwnerInput,
        wait: InputWait,
    },
    Setting {
        setting: OwnerSetting,
        wait: SettingWait,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerEvent {
    Command(SessionOwnerCommand),
    Wire(SessionWireResult),
    Store(SessionStoreResult),
    Publication {
        run: RunAttempt<LoadAttempt>,
        outcome: EffectOutcome,
    },
    DeadlineElapsed(SessionDeadline),
    RoutedTool(RoutedSessionToolEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionWireRequest {
    Restore {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        value: SettingValueId,
    },
    Close(RunAttempt<CloseAttempt>),
    Prompt(OwnerInput),
    Steer(OwnerInput),
    Cancel {
        generation: SessionGeneration,
        turn: TurnId,
    },
    Set {
        setting: OwnerSetting,
        value: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStoreRequest {
    ReadRestoreSetup(RunAttempt<LoadAttempt>),
    MarkDispatch(OwnerInput),
    UndoDispatch(OwnerInput),
    MarkSteerDispatch(OwnerInput),
    ReadDispatch(OwnerInput),
    RecordConfirmedSetting {
        setting: OwnerSetting,
        value: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionPublication {
    /// The Hub applies history and Idle in the order of owner commits.
    Activated {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    Closed(SessionGeneration),
    Unloaded(SessionGeneration),
    TurnStarted {
        generation: SessionGeneration,
        turn: TurnId,
    },
    TurnEnded {
        generation: SessionGeneration,
        turn: TurnId,
        idle: bool,
    },
    SettingObserved {
        setting: OwnerSetting,
        value: SettingValueId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteEffect {
    None,
    Applied,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseOutcome {
    Confirmed,
    Refused,
    OutcomeUnknown,
}

impl CloseOutcome {
    #[must_use]
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::Confirmed => "the provider confirmed the close",
            Self::Refused => "the provider refused the close",
            Self::OutcomeUnknown => {
                "the Router released the session locally with an unknown provider close outcome, so you can restore it again"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreFailureStage {
    Setup,
    Wire,
    Publication,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerReply {
    Restored {
        run: RunAttempt<LoadAttempt>,
        mode_confirmed: bool,
    },
    RestoreFailed {
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
    },
    CloseFinished {
        run: RunAttempt<CloseAttempt>,
        outcome: CloseOutcome,
    },
    InputFinished {
        input: OwnerInput,
        remote: RemoteEffect,
    },
    SettingFinished {
        setting: OwnerSetting,
        remote: RemoteEffect,
        saved: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerEffect {
    ArmDeadline(SessionDeadline),
    Wire(SessionWireRequest),
    Store(SessionStoreRequest),
    Publish(SessionPublication),
    Reply(SessionOwnerReply),
    RetireRoute(SessionGeneration),
    StageReplayTool {
        generation: SessionGeneration,
        tool_id: String,
    },
    ForwardTool {
        generation: SessionGeneration,
        tool_id: String,
    },
    CheckRetainedTool {
        generation: SessionGeneration,
        tool_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerRefusal {
    Cell(state_machine_core::TransitionError<SessionCellRefusal>),
    GenerationChanged,
    SettingsUnresolved,
    InputAlreadyAdmitted,
    EffectInProgress,
}

#[derive(Debug)]
pub struct SessionOwnerCommit {
    pub next: SessionOwnerState,
    pub effects: Vec<SessionOwnerEffect>,
}

#[derive(Debug)]
pub enum SessionOwnerRejection {
    Refused(SessionOwnerRefusal),
    /// Preserve the original stamped event for diagnostics at the caller.
    IgnoredLate,
}

pub type SessionOwnerDecision = Result<SessionOwnerCommit, SessionOwnerRejection>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingRestore {
    Setup {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    Wire {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    Mode {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    Publication {
        run: RunAttempt<LoadAttempt>,
        mode_confirmed: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingPrompt {
    input: OwnerInput,
    wait: InputWait,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOwnerState {
    cell: SessionCell,
    restore: Option<PendingRestore>,
    prompts: BTreeMap<u64, PendingPrompt>,
    steer: Option<PendingPrompt>,
    setting: Option<(OwnerSetting, SettingWait)>,
    tools: BTreeMap<String, ToolOwnership>,
}

impl SessionOwnerState {
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            cell: SessionCell::fresh(),
            restore: None,
            prompts: BTreeMap::new(),
            steer: None,
            setting: None,
            tools: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn lifecycle(&self) -> SessionLifecycle {
        self.cell.lifecycle
    }

    #[must_use]
    pub const fn turn(&self) -> SessionTurn {
        self.cell.turn
    }
}

#[cfg(test)]
#[path = "session_owner_test.rs"]
mod tests;
