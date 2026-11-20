use super::{
    InputId, OwnerInput, OwnerSetting, RunAttempt, SessionGeneration, SessionLifecycle,
    SessionOwnerCommand, SessionOwnerReply, SessionPublication, SessionStoreRequest,
    SessionStoreResult, SessionTurn, SessionWireRequest, SessionWireResult, TurnId,
};
use crate::ProviderSettingKind;
use crate::session_lifecycle_machine::{CloseAttempt, LoadAttempt};
use crate::session_setting_machine::SettingValueId;
use session_event_model::{CapabilityReport, SessionItem, SessionSettings};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnerIncarnation(pub Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ConnectionEpoch(pub Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerAddress {
    pub incarnation: OwnerIncarnation,
    pub connection_epoch: ConnectionEpoch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerMailboxEvent {
    pub owner: OwnerAddress,
    pub event: OwnerEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerEvent {
    Command(SessionOwnerCommand),
    ConnectionAttached,
    ConnectionLost,
    Create(CreateProgress),
    Wire(SessionWireResult),
    Dispatch(DispatchResult),
    Store(SessionStoreResult),
    Prepare(RestorePreparation),
    Publication(PublicationResult),
    Deadline(OwnerDeadline),
    RoutedTool(RoutedTool),
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DispatchAuthorization {
    pub incarnation: OwnerIncarnation,
    pub run: SessionGeneration,
    pub input_id: InputId,
    pub authorization_seq: u64,
    pub target: AuthorizationTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AuthorizationTarget {
    NewTurn,
    ExistingTurn { turn: TurnId },
    Session,
}

impl DispatchAuthorization {
    /// Check the route under the same transport lock as identity comparison
    /// and enqueue. A successful check alone does not authorize a wire send.
    #[must_use]
    pub fn matches_route(&self, run: SessionGeneration, current_turn: Option<TurnId>) -> bool {
        self.run == run
            && match self.target {
                AuthorizationTarget::ExistingTurn { turn } => current_turn == Some(turn),
                AuthorizationTarget::NewTurn | AuthorizationTarget::Session => true,
            }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorizedWork {
    Prompt(OwnerInput),
    Steer(OwnerInput),
    Cancel(OwnerInput),
    Setting {
        input_id: InputId,
        setting: OwnerSetting,
        value: SettingValueId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevokeReason {
    Cancelled,
    Closed,
    EnqueueExpired,
    ConnectionLost,
    TurnEnded,
}

impl RevokeReason {
    #[must_use]
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled before sending",
            Self::Closed => "the session closed first; send again",
            Self::EnqueueExpired => "the Router couldn't send in time; send again",
            Self::ConnectionLost => "the provider connection ended; reconnect and send again",
            Self::TurnEnded => "the turn you steered had already ended; send again",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    Taken(DispatchAuthorization),
    Revoked(DispatchAuthorization),
    AlreadyTaken(DispatchAuthorization),
    PromptRequired {
        authorization: DispatchAuthorization,
        replacement_turn: TurnId,
    },
    DefinitelyUnsent(DispatchAuthorization),
    StaleTarget(DispatchAuthorization),
    PromptFinished {
        authorization: DispatchAuthorization,
        outcome: super::WireOutcome<()>,
    },
    SteerFinished {
        authorization: DispatchAuthorization,
        outcome: super::WireOutcome<()>,
    },
    SettingFinished {
        authorization: DispatchAuthorization,
        observation: super::SettingObservation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnMembership {
    Provisional {
        run: SessionGeneration,
        turn: TurnId,
    },
    RemoveOnRevoke {
        run: SessionGeneration,
        turn: TurnId,
    },
    End {
        run: SessionGeneration,
        turn: TurnId,
    },
    Retire(SessionGeneration),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutedTool {
    Exact {
        run: SessionGeneration,
        turn: TurnId,
        tool_id: String,
    },
    ExactIdle {
        run: SessionGeneration,
        item_id: String,
    },
    Replay {
        run: RunAttempt<LoadAttempt>,
        item: SessionItem,
    },
    Observed {
        run: SessionGeneration,
        turn: Option<TurnId>,
        tool_id: String,
        observation: super::ToolObservation,
    },
    ItemRetained {
        run: SessionGeneration,
        item_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateProgress {
    ProviderId {
        operation: Uuid,
        provider_id: String,
    },
    Adopted {
        operation: Uuid,
    },
    AlreadyOwned {
        operation: Uuid,
    },
    Failed {
        operation: Uuid,
        remote: super::RemoteEffect,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestorePreparation {
    RouteReady(RunAttempt<LoadAttempt>),
    HostReady {
        run: RunAttempt<LoadAttempt>,
        capabilities: CapabilityReport,
        settings: SessionSettings,
        observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
    },
    FastOff {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        observation: SettingEvidence,
    },
    Failed {
        run: RunAttempt<LoadAttempt>,
        stage: PreparationStage,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationStage {
    SavedSetup,
    Route,
    HostSetup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePhase {
    Preflight,
    Attempted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreFailure {
    Lost,
    Refused { code: i64, safe_detail: String },
    ProviderError { code: i64 },
    Disconnected,
    NoAnswer,
    LocalPrerequisite { stage: PreparationStage },
    Closed,
}

impl RestoreFailure {
    #[must_use]
    pub const fn next_step(&self) -> &'static str {
        match self {
            Self::Lost => "the provider no longer has this session, so start a new session",
            Self::Refused { .. } => "check the provider's refusal before trying the restore again",
            Self::ProviderError { .. } => "check the provider error and retry the restore",
            Self::Disconnected => "reconnect to the provider and retry the restore",
            Self::NoAnswer => {
                "the restore effect is unknown, so retry the restore when the provider responds"
            }
            Self::LocalPrerequisite { .. } => {
                "retry the restore when the Router can prepare the session"
            }
            Self::Closed => "load or resume the session again if still needed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservedSetting {
    Confirmed(SettingValueId),
    LastConfirmed(SettingValueId),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationCause {
    NotOffered,
    Refused,
    NoAnswer,
    Unconfirmed,
    ReportedOther,
    FellBack,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingEvidence {
    pub value: ObservedSetting,
    pub cause: Option<ObservationCause>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreDisposition {
    Restored {
        earlier_unanswered: bool,
    },
    Retired {
        failure: RestoreFailure,
        remote: super::RemoteEffect,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreResult {
    pub attempt: RunAttempt<LoadAttempt>,
    pub phase: RestorePhase,
    pub disposition: RestoreDisposition,
    pub observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BatchId {
    pub incarnation: OwnerIncarnation,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationGap {
    Refused,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationStatus {
    Current,
    Degraded {
        since: BatchId,
        reason: PublicationGap,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    pub owner: OwnerAddress,
    pub lifecycle: SessionLifecycle,
    pub turn: SessionTurn,
    pub settings: BTreeMap<ProviderSettingKind, SettingEvidence>,
    pub publication: PublicationStatus,
    pub history: HistoryAvailability,
    pub last_restore: Option<RestoreResult>,
    pub last_live_turn: Option<TurnId>,
    pub queue: Vec<QueueEntrySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEntrySnapshot {
    pub sequence: u64,
    pub input: OwnerInput,
    pub started: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicationBatch {
    Commit {
        id: BatchId,
        facts: Vec<PublicationFact>,
    },
    Rebase {
        id: BatchId,
        baseline: Box<SessionSnapshot>,
        history: HistoryAvailability,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicationFact {
    ReplayBoundary(RunAttempt<LoadAttempt>),
    HistoryUnavailable(RunAttempt<LoadAttempt>),
    Replay {
        run: RunAttempt<LoadAttempt>,
        item: SessionItem,
    },
    Capabilities {
        run: SessionGeneration,
        report: CapabilityReport,
    },
    Settings {
        run: SessionGeneration,
        settings: SessionSettings,
        observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
    },
    Live(SessionPublication),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationResult {
    Applied(BatchId),
    Refused(BatchId),
    CapacityReturned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerDeadline {
    ReadSetup(RunAttempt<LoadAttempt>),
    RestoreWire(RunAttempt<LoadAttempt>),
    RestoreMode(RunAttempt<LoadAttempt>),
    RestoreFastOff(RunAttempt<LoadAttempt>),
    HostPrepare(RunAttempt<LoadAttempt>),
    DispatchMark { input: OwnerInput },
    UndoStore { input: OwnerInput },
    Close(RunAttempt<CloseAttempt>),
    SettingWire(OwnerSetting),
    SettingStore(OwnerSetting),
    TerminalStore(TerminalWriteId),
    Enqueue(DispatchAuthorization),
    SteerResult(DispatchAuthorization),
    Publication(BatchId),
    Rebase(BatchId),
    Create(Uuid),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerEffect {
    Arm {
        deadline: OwnerDeadline,
        budget: Duration,
    },
    Wire(SessionWireRequest),
    Authorize {
        authorization: DispatchAuthorization,
        work: AuthorizedWork,
    },
    /// The executor synchronously arbitrates with take and reports the
    /// result before processing the next mailbox event. It never awaits I/O.
    Revoke {
        authorization: DispatchAuthorization,
        reason: RevokeReason,
    },
    CallbackMembership(TurnMembership),
    Store(SessionStoreRequest),
    PrepareHost(RunAttempt<LoadAttempt>),
    PrepareRoute(RunAttempt<LoadAttempt>),
    Publish(PublicationBatch),
    Reply(SessionOwnerReply),
    ReplyUnsaved(SessionOwnerReply),
    RestoreSettled {
        operation: Uuid,
        result: RestoreResult,
    },
    AdoptProviderId {
        operation: Uuid,
        provider_id: String,
    },
    CreateSettled {
        operation: Uuid,
        outcome: CreateOutcome,
    },
    RetireRoute(SessionGeneration),
    ReopenRoute(RunAttempt<CloseAttempt>),
    CancelNotSent {
        authorization: DispatchAuthorization,
        reason: RevokeReason,
    },
    ForwardTool {
        run: SessionGeneration,
        turn: Option<TurnId>,
        tool_id: String,
    },
    CheckRetainedTool {
        run: SessionGeneration,
        tool_id: String,
    },
}

/// The mailbox validates this incarnation and epoch before applying any result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerOutbox {
    pub owner: OwnerAddress,
    pub effects: Vec<OwnerEffect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PendingPorts {
    pub address: OwnerAddress,
    pub connection_alive: bool,
    pub next_authorization: u64,
    pub authorizations: BTreeMap<DispatchAuthorization, PendingAuthorization>,
    pub admitted_inputs: BTreeSet<InputId>,
    pub next_batch: u64,
    pub latest_batch: Option<BatchId>,
    pub awaiting_apply: BTreeSet<BatchId>,
    pub rebase: Option<BatchId>,
    pub publication: PublicationStatus,
    pub history: HistoryAvailability,
    pub last_restore: Option<RestoreResult>,
    pub replay: Vec<SessionItem>,
    pub retained_items: BTreeSet<String>,
    pub observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
    pub last_live_turn: Option<TurnId>,
    pub restore_dependents: BTreeMap<Uuid, RunAttempt<LoadAttempt>>,
    pub create: Option<Uuid>,
    pub adopting: bool,
    pub setup: Option<PreparedSetup>,
    pub route_ready: bool,
    pub latest_setting: BTreeMap<ProviderSettingKind, OwnerSetting>,
    pub restore_setting_fences:
        BTreeMap<ProviderSettingKind, (RunAttempt<LoadAttempt>, OwnerSetting)>,
    pub fast_reapply: Option<OwnerSetting>,
    pub close_support: Option<(RunAttempt<CloseAttempt>, bool)>,
    pub steer_replacement: Option<OwnerInput>,
    pub unsent_prompts: BTreeMap<OwnerInput, RevokeReason>,
    pub pending_terminal: BTreeMap<TerminalWriteId, SessionOwnerReply>,
    pub terminal_sequence: u64,
}

impl PendingPorts {
    pub fn new(address: OwnerAddress) -> Self {
        Self {
            address,
            connection_alive: true,
            next_authorization: 0,
            authorizations: BTreeMap::new(),
            admitted_inputs: BTreeSet::new(),
            next_batch: 0,
            latest_batch: None,
            awaiting_apply: BTreeSet::new(),
            rebase: None,
            publication: PublicationStatus::Current,
            history: HistoryAvailability::Unavailable,
            last_restore: None,
            replay: Vec::new(),
            retained_items: BTreeSet::new(),
            observed: BTreeMap::new(),
            last_live_turn: None,
            restore_dependents: BTreeMap::new(),
            create: None,
            adopting: false,
            setup: None,
            route_ready: false,
            latest_setting: BTreeMap::new(),
            restore_setting_fences: BTreeMap::new(),
            fast_reapply: None,
            close_support: None,
            steer_replacement: None,
            unsent_prompts: BTreeMap::new(),
            pending_terminal: BTreeMap::new(),
            terminal_sequence: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PreparedSetup {
    pub capabilities: CapabilityReport,
    pub settings: SessionSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum AuthorizationStatus {
    Offered,
    Revoking(RevokeReason),
    Taken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PendingAuthorization {
    pub work: AuthorizedWork,
    pub status: AuthorizationStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateOutcome {
    Adopted(SessionGeneration),
    SessionIdInUse,
    Failed(super::RemoteEffect),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseRequest {
    pub run: RunAttempt<CloseAttempt>,
    pub provider_supports_close: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TerminalWriteId {
    pub incarnation: OwnerIncarnation,
    pub sequence: u64,
}
