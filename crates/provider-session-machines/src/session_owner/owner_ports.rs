use super::{
    InputId, LastLiveTurn, OwnerInput, OwnerSetting, PromptCompletion, RunAttempt,
    SessionGeneration, SessionLifecycle, SessionOwnerCommand, SessionOwnerReply,
    SessionPublication, SessionStoreRequest, SessionStoreResult, SessionTurn, SessionWireRequest,
    SessionWireResult, TurnId,
};
use crate::session_lifecycle_machine::{CloseAttempt, LoadAttempt};
use crate::session_setting_machine::{SettingApplication, SettingValueId};
use crate::{ProviderSettingKind, ProviderSettingsCatalog};
use session_event_model::{
    CapabilityReport, SessionEvent, SessionItem, SessionSettings, SessionState, SkillView,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
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
    DeliveryEvidence {
        placement: DeliveryPlacement,
        outcome: super::EffectOutcome,
    },
    ConnectionAttached,
    ConnectionLost,
    RunLost(SessionGeneration),
    Create(CreateProgress),
    Wire(SessionWireResult),
    Dispatch(DispatchResult),
    Store(SessionStoreResult),
    Prepare(RestorePreparation),
    Publication(PublicationResult),
    Deadline(OwnerDeadline),
    RoutedTool(RoutedTool),
    Stream {
        run: SessionGeneration,
        expected_turn: Option<TurnId>,
        event: SessionEvent,
    },
    SubmittedInput {
        input: OwnerInput,
        item: session_event_model::SessionItem,
    },
    Replay {
        run: RunAttempt<LoadAttempt>,
        events: Vec<SessionEvent>,
    },
    SkillView {
        run: SessionGeneration,
        view: SkillView,
    },
    ObservedTurnEnded {
        run: SessionGeneration,
        turn: TurnId,
        outcome: session_event_model::TurnOutcome,
    },
    OutputLimit {
        run: SessionGeneration,
        turn: TurnId,
    },
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
    EarlierOutcomeUnknown { operation: Uuid },
    RestoreDidNotActivate,
    NotLoaded,
    QueueCancelled,
    QueueClosed,
    QueueConnectionLost,
    DeliveryEvidenceUnavailable,
    SkillSelectionChanged,
    SettingsUnresolved,
    RunReplaced,
    Cancelled,
    Closed,
    EnqueueExpired,
    ConnectionLost,
    TurnEnded,
    ProviderSteerRefused,
}

impl RevokeReason {
    #[must_use]
    pub const fn queue_not_sent_reason(self) -> Option<crate::queued_entry_machine::NotSentReason> {
        use crate::queued_entry_machine::NotSentReason;
        match self {
            Self::QueueCancelled => Some(NotSentReason::CancelledFromQueue),
            Self::QueueClosed => Some(NotSentReason::SessionClosed),
            Self::QueueConnectionLost => Some(NotSentReason::ProviderConnectionClosed),
            _ => None,
        }
    }

    #[must_use]
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::EarlierOutcomeUnknown { .. } => "not sent: an earlier outcome is unknown",
            Self::RestoreDidNotActivate => {
                "the restore did not activate; load or resume this session and retry the input"
            }
            Self::NotLoaded => "session is not loaded; load it or resume it",
            Self::QueueCancelled => "not sent: cancelled from the queue",
            Self::QueueClosed => "not sent: session closed",
            Self::QueueConnectionLost => "not sent: provider connection closed",
            Self::DeliveryEvidenceUnavailable => {
                "the Router couldn't record delivery evidence; send again"
            }
            Self::SkillSelectionChanged => "invalid or stale skill selection",
            Self::SettingsUnresolved => {
                "settings-uncertain: run conversation settings set to resolve the unknown provider setting outcome"
            }
            Self::RunReplaced => "the session was restored again; send again if still needed",
            Self::Cancelled => "cancelled before sending",
            Self::Closed => "the session closed first; send again",
            Self::EnqueueExpired => "the Router couldn't send in time; send again",
            Self::ConnectionLost => "the provider connection ended; reconnect and send again",
            Self::TurnEnded => "the turn you steered had already ended; send again",
            Self::ProviderSteerRefused => "steerFailed",
        }
    }

    #[must_use]
    pub fn display_detail(self) -> std::borrow::Cow<'static, str> {
        if let Self::EarlierOutcomeUnknown { operation } = self {
            return format!("an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {operation}").into();
        }
        self.human_phrase().into()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    CancelDrainFinished(DispatchAuthorization),
    CancelTaken {
        authorization: DispatchAuthorization,
        drain: CancelDrainReport,
    },
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
        outcome: super::WireOutcome<PromptCompletion>,
    },
    PromptFailed {
        authorization: DispatchAuthorization,
        failure: ProviderWireFailure,
    },
    SteerFinished {
        authorization: DispatchAuthorization,
        outcome: super::WireOutcome<()>,
    },
    SteerInvalidReply(DispatchAuthorization),
    SteerStartedNewTurn {
        authorization: DispatchAuthorization,
        turn: TurnId,
    },
    SettingFinished {
        authorization: DispatchAuthorization,
        observation: super::SettingObservation,
    },
    SettingKept {
        authorization: DispatchAuthorization,
        reason: Option<PermissionModeReason>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PendingTurnResponse {
    pub method: &'static str,
    pub request_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CancelDrainReport {
    Drained,
    Expired { pending: Vec<PendingTurnResponse> },
}

impl CancelDrainReport {
    pub const EXPIRED_SUMMARY: &'static str =
        "cancel sent; some pending approval replies may reach the harness after it";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnMembership {
    Provisional {
        run: SessionGeneration,
        turn: TurnId,
    },
    ObservedEligible {
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateProgress {
    ProviderId {
        operation: Uuid,
        provider_id: String,
    },
    Adopted {
        operation: Uuid,
        facts: Box<CreatedSessionFacts>,
    },
    AlreadyOwned {
        operation: Uuid,
    },
    Failed {
        operation: Uuid,
        remote: super::RemoteEffect,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CreatedSessionFacts {
    pub capabilities: CapabilityReport,
    pub settings: SessionSettings,
    pub observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
    pub mode_reason: Option<PermissionModeReason>,
    pub keep_fast_off: bool,
    pub failed_setting: Option<InitialSettingFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSettingFailure {
    pub kind: ProviderSettingKind,
    pub attempted: SettingValueId,
    pub application: SettingApplication,
    pub unknown: bool,
    pub confirmed_current: bool,
}

/// Explains how the provider's confirmed mode relates to requested Auto.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionModeReason {
    AutoNotOffered,
    AgentFellBack,
    AutoNotRealized,
    EscalationUnproven,
    AutoNotApplied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingReadback {
    pub value: SettingValueId,
    pub settings: SessionSettings,
    pub fast: FastReadback,
    pub mode_reason: Option<PermissionModeReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FastReadback {
    NotOffered,
    Off,
    On {
        application: SettingApplication,
        input: InputId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelFastOutcome {
    ConfirmedOff,
    NotOffered,
    Refused,
    Unconfirmed,
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
        catalogue: Option<Arc<ProviderSettingsCatalog>>,
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
    Lost {
        reported: Option<ProviderWireFailure>,
    },
    Refused(ProviderWireFailure),
    ProviderError(ProviderWireFailure),
    NotEnqueued,
    Disconnected,
    NoAnswer,
    LocalPrerequisite {
        stage: PreparationStage,
    },
    Closed,
}

impl RestoreFailure {
    #[must_use]
    pub const fn from_coded(failure: ProviderWireFailure) -> Self {
        match failure.kind {
            ProviderWireFailureKind::SessionNotFound
            | ProviderWireFailureKind::ResourceNotFound => Self::Lost {
                reported: Some(failure),
            },
            ProviderWireFailureKind::RestoreFailed
            | ProviderWireFailureKind::Rejected
            | ProviderWireFailureKind::RequestCancelled => Self::ProviderError(failure),
            _ => Self::Refused(failure),
        }
    }

    #[must_use]
    pub const fn coded_evidence(&self) -> Option<ProviderWireFailure> {
        match self {
            Self::Lost { reported } => *reported,
            Self::Refused(evidence) | Self::ProviderError(evidence) => Some(*evidence),
            _ => None,
        }
    }

    #[must_use]
    pub const fn next_step(&self) -> &'static str {
        match self {
            Self::Lost { .. } => {
                "the provider no longer knows this session, so start a new session with the same setup"
            }
            Self::ProviderError(_) => {
                "the provider failed while restoring this session, so retry it"
            }
            Self::Refused(_) => {
                "the provider refused to restore this session, so check the provider before retrying"
            }
            Self::NotEnqueued => "the Router couldn't send the restore, so retry this session",
            Self::Disconnected => {
                "the provider disconnected before confirming the restore, so reconnect and retry this session"
            }
            Self::NoAnswer => {
                "the provider did not answer the restore in time, so retry this session"
            }
            Self::LocalPrerequisite { .. } => {
                "retry the restore when the Router can prepare the session"
            }
            Self::Closed => "load or resume the session again if still needed",
        }
    }
}

/// Correlation for a coded reply, with no provider-controlled message text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderWireFailure {
    pub kind: ProviderWireFailureKind,
    pub code: i64,
    pub correlation_id: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderWireFailureKind {
    AuthenticationRequired,
    SessionNotFound,
    ResourceNotFound,
    UnsupportedMethod,
    InvalidParams,
    RequestCancelled,
    Rejected,
    RestoreFailed,
    RestoreRefused,
}

impl ProviderWireFailure {
    #[must_use]
    pub const fn definitely_no_effect(self) -> bool {
        matches!(
            self.kind,
            ProviderWireFailureKind::AuthenticationRequired
                | ProviderWireFailureKind::SessionNotFound
                | ProviderWireFailureKind::ResourceNotFound
                | ProviderWireFailureKind::UnsupportedMethod
                | ProviderWireFailureKind::InvalidParams
        )
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

impl RestoreResult {
    #[must_use]
    pub const fn replay_warning(
        &self,
        via: crate::session_lifecycle_machine::RestoreVia,
    ) -> Option<&'static str> {
        if matches!(
            (via, &self.disposition),
            (
                crate::session_lifecycle_machine::RestoreVia::Load,
                RestoreDisposition::Restored {
                    earlier_unanswered: true
                }
            )
        ) {
            Some("an earlier load timed out; replayed history may include its late events")
        } else {
            None
        }
    }
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
    pub foreground: Option<OwnerInput>,
    pub recent_steer: Option<OwnerInput>,
    pub settings: BTreeMap<ProviderSettingKind, SettingEvidence>,
    pub mode_reason: Option<PermissionModeReason>,
    pub settings_block_input: bool,
    pub publication: PublicationStatus,
    pub history: HistoryAvailability,
    pub last_restore: Option<RestoreResult>,
    pub last_live_turn: Option<LastLiveTurn>,
    pub cancel_unconfirmed: Option<OwnerInput>,
    pub provider_started_turn: Option<OwnerInput>,
    pub projection: SessionProjection,
    pub queue: Vec<QueueEntrySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProjection {
    pub state: SessionState,
    pub capabilities: Option<CapabilityReport>,
    pub settings: Option<SessionSettings>,
    pub skills: SkillView,
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
        event: SessionEvent,
    },
    Capabilities {
        run: SessionGeneration,
        report: CapabilityReport,
    },
    Settings {
        run: SessionGeneration,
        settings: SessionSettings,
        observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
        catalogue: Option<Arc<ProviderSettingsCatalog>>,
    },
    Live(SessionPublication),
    Stream {
        run: SessionGeneration,
        event: SessionEvent,
    },
    Skills {
        run: SessionGeneration,
        view: SkillView,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationResult {
    Applied(BatchId),
    Refused(BatchId),
    CapacityReturned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerDeadline {
    DeliveryEvidence(DeliveryPlacement),
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
    CancelDrain(DispatchAuthorization),
    CancelConfirmation(DispatchAuthorization),
    SteerResult(DispatchAuthorization),
    Publication(BatchId),
    Rebase(BatchId),
    Create(Uuid),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerEffect {
    RecordDeliveryEvidence(DeliveryPlacement),
    Arm {
        deadline: OwnerDeadline,
        budget: Duration,
    },
    Wire(SessionWireRequest),
    Authorize {
        authorization: DispatchAuthorization,
        work: AuthorizedWork,
    },
    DrainCancel(DispatchAuthorization),
    TakeCancel {
        authorization: DispatchAuthorization,
        expired: bool,
    },
    CancelSent {
        authorization: DispatchAuthorization,
        operation: Uuid,
        drain: CancelDrainReport,
    },
    /// The executor synchronously arbitrates with take and reports the
    /// result before processing the next mailbox event. It never awaits I/O.
    Revoke {
        authorization: DispatchAuthorization,
        reason: RevokeReason,
    },
    CallbackMembership(TurnMembership),
    Store(SessionStoreRequest),
    PrepareHost {
        run: RunAttempt<LoadAttempt>,
        catalogue: Arc<ProviderSettingsCatalog>,
    },
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
        operation: Uuid,
        reason: RevokeReason,
    },
    InputQueued(OwnerInput),
    SettingQueued {
        input: InputId,
        setting: OwnerSetting,
    },
    PromptTaken(OwnerInput),
    SteerSubmitted(OwnerInput),
    ProviderStartedTurn(OwnerInput),
    ForwardTool {
        run: SessionGeneration,
        turn: Option<TurnId>,
        tool_id: String,
    },
}

/// Placement stamped before Host evidence or any durable dispatch mark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryPlacement {
    pub owner: OwnerAddress,
    pub input: OwnerInput,
    pub kind: DeliveryPlacementKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryPlacementKind {
    Start,
    Steer,
    Queue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum DeliveryEvidenceState {
    Waiting,
    Offered(DeliveryPlacement),
    Recorded(DeliveryPlacementKind),
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
    pub selected_skills: BTreeMap<InputId, Vec<session_event_model::SelectedSkill>>,
    pub delivery_evidence: BTreeMap<InputId, DeliveryEvidenceState>,
    pub next_batch: u64,
    pub latest_batch: Option<BatchId>,
    pub awaiting_apply: BTreeSet<BatchId>,
    pub rebase: Option<BatchId>,
    pub publication: PublicationStatus,
    pub history: HistoryAvailability,
    pub last_restore: Option<RestoreResult>,
    pub replay: Vec<SessionEvent>,
    pub observed: BTreeMap<ProviderSettingKind, SettingEvidence>,
    pub settings_catalogue: Option<Arc<ProviderSettingsCatalog>>,
    pub mode_reason: Option<PermissionModeReason>,
    pub last_live_turn: Option<LastLiveTurn>,
    pub projection: super::owner_reducer::OwnerProjection,
    pub restore_dependents: BTreeMap<Uuid, RunAttempt<LoadAttempt>>,
    pub create: Option<Uuid>,
    pub adopting: bool,
    pub setup: Option<PreparedSetup>,
    pub route_ready: bool,
    pub latest_setting: BTreeMap<ProviderSettingKind, OwnerSetting>,
    pub queued_settings: VecDeque<QueuedSetting>,
    pub model_fast_after: Option<(OwnerSetting, OwnerSetting)>,
    pub restore_setting_fences:
        BTreeMap<ProviderSettingKind, (RunAttempt<LoadAttempt>, OwnerSetting)>,
    pub fast_reapply: Option<OwnerSetting>,
    pub close_support: Option<(RunAttempt<CloseAttempt>, bool)>,
    pub steer_replacement: Option<OwnerInput>,
    pub provider_started_turn: Option<OwnerInput>,
    pub previous_wire_prompts: BTreeMap<OwnerInput, super::owner_reducer::PreviousPromptSettlement>,
    pub recent_steer: Option<OwnerInput>,
    pub cancel_confirmation: Option<super::owner_reducer::PendingCancelConfirmation>,
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
            selected_skills: BTreeMap::new(),
            delivery_evidence: BTreeMap::new(),
            next_batch: 0,
            latest_batch: None,
            awaiting_apply: BTreeSet::new(),
            rebase: None,
            publication: PublicationStatus::Current,
            history: HistoryAvailability::Unavailable,
            last_restore: None,
            replay: Vec::new(),
            observed: BTreeMap::new(),
            settings_catalogue: None,
            mode_reason: None,
            last_live_turn: None,
            projection: super::owner_reducer::OwnerProjection::default(),
            restore_dependents: BTreeMap::new(),
            create: None,
            adopting: false,
            setup: None,
            route_ready: false,
            latest_setting: BTreeMap::new(),
            queued_settings: VecDeque::new(),
            model_fast_after: None,
            restore_setting_fences: BTreeMap::new(),
            fast_reapply: None,
            close_support: None,
            steer_replacement: None,
            provider_started_turn: None,
            previous_wire_prompts: BTreeMap::new(),
            recent_steer: None,
            cancel_confirmation: None,
            unsent_prompts: BTreeMap::new(),
            pending_terminal: BTreeMap::new(),
            terminal_sequence: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct QueuedSetting {
    pub input: InputId,
    pub setting: OwnerSetting,
    pub value: SettingValueId,
    pub expected_model: Option<SettingValueId>,
    pub context: Option<TurnSettingContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct TurnSettingContext {
    pub reuse_confirmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PreparedSetup {
    pub capabilities: CapabilityReport,
    pub settings: SessionSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum AuthorizationStatus {
    DrainingCancel,
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
