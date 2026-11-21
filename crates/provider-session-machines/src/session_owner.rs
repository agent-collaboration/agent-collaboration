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
use crate::{ProviderSettingKind, ProviderSettingsCatalog, SessionGeneration};
use session_event_model::{InputId, LocalCause, StopReason, TurnOutcome};
use std::collections::BTreeMap;
use std::sync::Arc;

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
    DeliveryAuto { steering_supported: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryInput {
    Prompt {
        admission: PromptAdmission,
        selected: Vec<session_event_model::SelectedSkill>,
    },
    Steer {
        supported: bool,
    },
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
    PromptWithSkills {
        input: OwnerInput,
        admission: PromptAdmission,
        selected: Vec<session_event_model::SelectedSkill>,
    },
    Steer {
        input: OwnerInput,
        supported: bool,
    },
    SteerExisting {
        input: OwnerInput,
        supported: bool,
    },
    PlaceDelivery {
        input: OwnerInput,
        delivery: DeliveryInput,
    },
    QueueColdDelivery {
        input: OwnerInput,
        attempt: LoadAttempt,
        selected: Vec<session_event_model::SelectedSkill>,
        record_evidence: bool,
    },
    CancelTurn {
        generation: SessionGeneration,
        turn: TurnId,
    },
    CancelQueued {
        generation: SessionGeneration,
        input: InputId,
    },
    CancelOperation {
        target: uuid::Uuid,
    },
    CancelSetting {
        input: InputId,
        setting: OwnerSetting,
    },
    Set {
        input: InputId,
        setting: OwnerSetting,
        value: SettingValueId,
        expected_model: Option<SettingValueId>,
    },
    SetInContext {
        input: InputId,
        setting: OwnerSetting,
        value: SettingValueId,
        expected_model: Option<SettingValueId>,
        reuse_confirmed: bool,
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
pub struct PromptCompletion {
    pub stop_reason: StopReason,
    pub local_cause: Option<LocalCause>,
    pub response: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastLiveTurn {
    pub turn: TurnId,
    pub input: InputId,
    pub outcome: TurnOutcome,
    pub observed_at_ms: i64,
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
pub struct RestoreLoadReadback {
    pub catalogue: Arc<ProviderSettingsCatalog>,
    pub router_auto_requested: bool,
    pub mode: RestoredMode,
    pub fast_on: Option<SettingApplication>,
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
    AppliedWithReadback(Box<SettingReadback>),
    Refused {
        current: Option<SettingValueId>,
        catalogue: Option<Arc<ProviderSettingsCatalog>>,
    },
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionWireResult {
    Restore {
        run: RunAttempt<LoadAttempt>,
        outcome: WireOutcome<RestoreLoadReadback>,
    },
    RestoreFailed {
        run: RunAttempt<LoadAttempt>,
        failure: RestoreFailure,
    },
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        outcome: RestoreModeOutcome,
        catalogue: Option<Arc<ProviderSettingsCatalog>>,
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
    DispatchBlockedByUnknown {
        input: OwnerInput,
        operation: uuid::Uuid,
    },
    QueuedDispatchBusy {
        input: OwnerInput,
        attempt: uuid::Uuid,
    },
    QueuedDispatchReady {
        input: OwnerInput,
        attempt: uuid::Uuid,
    },
    LateCancelledPromptReconciled {
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
        catalogue: Arc<ProviderSettingsCatalog>,
    },
    RestoreFastOff {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        catalogue: Arc<ProviderSettingsCatalog>,
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
    MarkQueuedDispatch(OwnerInput),
    UndoDispatch(OwnerInput),
    MarkSteerDispatch(OwnerInput),
    ReconcileLateCancelledPrompt(OwnerInput),
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
        input: InputId,
    },
    TurnEnded {
        generation: SessionGeneration,
        turn: TurnId,
        outcome: TurnOutcome,
        idle: bool,
    },
    PreviousTurnEnded {
        generation: SessionGeneration,
        turn: TurnId,
        outcome: TurnOutcome,
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
    PromptCompleted {
        input: OwnerInput,
        completion: PromptCompletion,
    },
    PromptFailed {
        input: OwnerInput,
        remote: RemoteEffect,
        failure: ProviderWireFailure,
    },
    PromptCancelUnconfirmed {
        input: OwnerInput,
    },
    SteerStarted {
        input: OwnerInput,
        turn: TurnId,
    },
    SteerInvalidReply {
        input: OwnerInput,
    },
    SettingFinished {
        setting: OwnerSetting,
        remote: RemoteEffect,
        saved: bool,
    },
    SettingKept {
        setting: OwnerSetting,
    },
    ModelFastFinished {
        setting: OwnerSetting,
        fast: ModelFastOutcome,
    },
    SettingNotSent {
        setting: OwnerSetting,
        refusal: SessionOwnerRefusal,
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
    PrepareHost {
        run: RunAttempt<LoadAttempt>,
        catalogue: Arc<ProviderSettingsCatalog>,
    },
    ForwardTool {
        generation: SessionGeneration,
        tool_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingEffectInProgress {
    SettingInFlight(Option<crate::session_setting_machine::SessionSetting>),
    PromptInProgress,
    FencedSetting(Option<crate::session_setting_machine::SessionSetting>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectInProgress {
    TurnInProgress,
    PromptDispatch,
    QueuedPrompts,
    SettingChange,
    Steer,
    Cancel,
    Create,
    ProviderDisconnected,
    CreateOnActiveOwner,
    SettingCancelTooLate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOwnerRefusal {
    Cell(state_machine_core::TransitionError<SessionCellRefusal>),
    GenerationChanged,
    SettingsUnresolved,
    SettingContextChanged,
    InputAlreadyAdmitted,
    EffectInProgress(EffectInProgress),
    SettingEffectInProgress(SettingEffectInProgress),
    CancelTargetInactive,
    QueueFull,
    CannotSteer,
    SkillSelectionChanged,
}

impl SessionOwnerRefusal {
    #[must_use]
    pub const fn lifecycle_refusal(
        &self,
    ) -> Option<crate::session_lifecycle_machine::SessionLifecycleRefusal> {
        match self {
            Self::Cell(state_machine_core::TransitionError::Domain(
                SessionCellRefusal::Lifecycle(refusal),
            )) => Some(*refusal),
            _ => None,
        }
    }

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
struct SessionOwnerCommit {
    next: SessionOwnerState,
    effects: OwnerOutbox,
}

#[derive(Debug)]
pub enum SessionOwnerRejection {
    Refused(SessionOwnerRefusal),
    /// Preserve the original stamped event for diagnostics at the caller.
    IgnoredLate,
}

type SessionOwnerDecision = Result<SessionOwnerCommit, SessionOwnerRejection>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum PendingRestore {
    Setup {
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        after_queue_evidence: Option<uuid::Uuid>,
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
        setting: OwnerSetting,
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
    choose_steer_if_a_turn_starts: bool,
    queued: bool,
    parked_mark: Option<uuid::Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionOwnerState {
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

/// The reducer's task owns this machine. Consumers use committed effects and
/// snapshots without borrowing the reducer's state.
pub struct SessionOwnerMachine {
    state: SessionOwnerState,
}

impl SessionOwnerMachine {
    #[must_use]
    pub fn new(address: OwnerAddress) -> Self {
        Self {
            state: SessionOwnerState::new(address),
        }
    }

    pub fn handle(
        &mut self,
        event: OwnerMailboxEvent,
        observed_at_ms: i64,
    ) -> Result<OwnerOutbox, SessionOwnerRejection> {
        let commit = self.state.decide_at(event, observed_at_ms)?;
        self.state = commit.next;
        Ok(commit.effects)
    }

    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        self.state.snapshot()
    }

    #[must_use]
    pub fn has_pending_deadline(&self, deadline: &OwnerDeadline) -> bool {
        self.state.has_pending_deadline(deadline)
    }
}

#[cfg(test)]
#[path = "session_owner_test.rs"]
mod tests;
