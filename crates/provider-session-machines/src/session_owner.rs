//! Decisions and ordered effects for one provider session across its runs.
//! The caller executes effects and returns stamped observations as events.

mod owner_ports;
mod owner_reducer;
pub use owner_ports::*;

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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnerInput {
    pub generation: SessionGeneration,
    pub operation: uuid::Uuid,
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
    Create {
        operation: uuid::Uuid,
    },
    Restore {
        attempt: LoadAttempt,
        via: RestoreVia,
    },
    Close {
        generation: SessionGeneration,
        attempt: CloseAttempt,
    },
    CloseWithCapability(CloseRequest),
    DependOnRestore {
        operation: uuid::Uuid,
        run: RunAttempt<LoadAttempt>,
    },
    WarmPrompt(OwnerInput),
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
    CancelSetting {
        input: InputId,
        setting: OwnerSetting,
    },
    Set {
        input: InputId,
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
    NotOffered {
        current: Option<SettingValueId>,
    },
    Reapply {
        application: SettingApplication,
        value: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreModeOutcome {
    Confirmed(SettingValueId),
    Refused { current: Option<SettingValueId> },
    ReportedOther(SettingValueId),
    FellBack(SettingValueId),
    Unconfirmed,
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
        fast_on: Option<SettingApplication>,
    },
    RestoreFailed {
        run: RunAttempt<LoadAttempt>,
        failure: RestoreFailure,
    },
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        outcome: RestoreModeOutcome,
    },
    Close {
        run: RunAttempt<CloseAttempt>,
        outcome: WireOutcome<()>,
    },
    CloseNotSent(RunAttempt<CloseAttempt>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectOutcome {
    Applied,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStoreResult {
    Terminal {
        write: TerminalWriteId,
        outcome: EffectOutcome,
    },
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum RoutedSessionToolEvent {
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
pub(in crate::session_owner) enum RestoreWait {
    Setup,
    Wire,
    Mode,
    FastOff,
    Preparation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum InputWait {
    Admission,
    DispatchStore,
    UndoStore,
    Enqueue,
    Wire,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum SettingWait {
    Enqueue,
    Wire,
    Store,
}

/// The executor delivers a firing for the exact wait it armed on the
/// injected clock. The reducer never reads time or invents deadline values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum SessionDeadline {
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
pub enum SessionWireRequest {
    Create {
        operation: uuid::Uuid,
    },
    Restore {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
    },
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        value: SettingValueId,
    },
    RestoreFastOff {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
    },
    Close(RunAttempt<CloseAttempt>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStoreRequest {
    RecordTerminal {
        write: TerminalWriteId,
        reply: Box<SessionOwnerReply>,
    },
    ReadRestoreSetup(RunAttempt<LoadAttempt>),
    MarkDispatch(OwnerInput),
    UndoDispatch(OwnerInput),
    MarkSteerDispatch(OwnerInput),
    RecordConfirmedSetting {
        setting: OwnerSetting,
        value: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionPublication {
    Created(SessionGeneration),
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
    RouterOnly,
    DrainExpired,
    NotEnqueued,
    RefusedLive,
    RefusedLoading,
    OutcomeUnknown,
}

impl CloseOutcome {
    #[must_use]
    pub const fn remote_effect(self) -> RemoteEffect {
        match self {
            Self::Confirmed => RemoteEffect::Applied,
            Self::RouterOnly
            | Self::DrainExpired
            | Self::NotEnqueued
            | Self::RefusedLive
            | Self::RefusedLoading => RemoteEffect::None,
            Self::OutcomeUnknown => RemoteEffect::Unknown,
        }
    }

    #[must_use]
    pub const fn locally_retired(self) -> bool {
        !matches!(self, Self::RefusedLive)
    }

    #[must_use]
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::Confirmed => "closed",
            Self::RouterOnly => {
                "closed by the Router; this harness has no close, so it wasn't told"
            }
            Self::DrainExpired => {
                "the close wasn't sent because the running turn didn't stop in time; the Router released the session locally, and the harness may still be running that turn"
            }
            Self::NotEnqueued => {
                "the Router couldn't send the close; it released the session locally (the harness wasn't told), and you can restore it"
            }
            Self::RefusedLive => "the close was refused; the session is still open",
            Self::RefusedLoading => {
                "the close was refused while the session was loading; the Router released it locally, and you can restore it"
            }
            Self::OutcomeUnknown => {
                "the close may or may not have reached the harness; the Router released the session locally, and you can restore it"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum RestoreFailureStage {
    Setup,
    Wire,
    Preparation,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerReply {
    RestoreFinished(RestoreResult),
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
    InputUnsent {
        input: OwnerInput,
        reason: RevokeReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum SessionOwnerEffect {
    ArmDeadline(SessionDeadline),
    Wire(SessionWireRequest),
    Store(SessionStoreRequest),
    Publish(SessionPublication),
    Reply(SessionOwnerReply),
    RetireRoute(SessionGeneration),
    PrepareHost(RunAttempt<LoadAttempt>),
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

impl SessionOwnerRefusal {
    #[must_use]
    pub const fn retry_advice(&self) -> Option<&'static str> {
        match self {
            Self::GenerationChanged => {
                Some("the session was restored again; send again if still needed")
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct SessionOwnerCommit {
    pub next: SessionOwnerState,
    pub effects: OwnerOutbox,
}

#[derive(Debug)]
pub enum SessionOwnerRejection {
    Refused(SessionOwnerRefusal),
    /// Preserve the original stamped event for diagnostics at the caller.
    IgnoredLate,
}

pub type SessionOwnerDecision = Result<SessionOwnerCommit, SessionOwnerRejection>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum PendingRestore {
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
    FastOff {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        mode_confirmed: bool,
    },
    Preparing {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        mode_confirmed: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PendingPrompt {
    input: OwnerInput,
    wait: InputWait,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOwnerState {
    pub(in crate::session_owner) cell: SessionCell,
    pub(in crate::session_owner) restore: Option<PendingRestore>,
    pub(in crate::session_owner) prompts: BTreeMap<u64, PendingPrompt>,
    pub(in crate::session_owner) steer: Option<PendingPrompt>,
    pub(in crate::session_owner) setting: Option<(OwnerSetting, SettingWait)>,
    pub(in crate::session_owner) tools: BTreeMap<String, ToolOwnership>,
    pub(in crate::session_owner) ports: owner_ports::PendingPorts,
}

impl SessionOwnerState {
    #[must_use]
    pub fn new(address: OwnerAddress) -> Self {
        Self {
            cell: SessionCell::fresh(),
            restore: None,
            prompts: BTreeMap::new(),
            steer: None,
            setting: None,
            tools: BTreeMap::new(),
            ports: owner_ports::PendingPorts::new(address),
        }
    }

    #[must_use]
    pub(in crate::session_owner) const fn lifecycle(&self) -> SessionLifecycle {
        self.cell.lifecycle
    }

    #[must_use]
    pub(in crate::session_owner) const fn turn(&self) -> SessionTurn {
        self.cell.turn
    }
}

#[cfg(test)]
#[path = "session_owner_test.rs"]
mod tests;
