//! Stored definitions own configuration; their separate row owns timing progress.
use crate::automation_record_header::AutomationRecordHeader;
use crate::automation_store_error::{AutomationStoreError, AutomationStoreResult};
use collaboration_protocol::{
    AutomationId, AutomationKind, AutomationTiming, ChangeId, CodexGeneration, ContentReference,
    FiringId, FiringMessageOrigin, GridTime, MessageDelivery, ObservationTimestamp,
    ScheduleDefinition, SessionRef, SuspendedScheduleConfiguration, TimingRequest, WakeCreator,
};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BoundAutomationCreator {
    pub installation: ServiceId,
    pub declared: WakeCreator,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum StoredAutomationDefinition {
    Schedule(StoredScheduleDefinition),
    Wake(StoredWakeDefinition),
    Suspended(StoredSuspendedDefinition),
}

impl StoredAutomationDefinition {
    pub fn kind(&self) -> AutomationKind {
        match self {
            Self::Schedule(_) => AutomationKind::Schedule,
            Self::Wake(_) => AutomationKind::Wake,
            Self::Suspended(definition) => match definition.configuration {
                StoredSuspendedConfiguration::Schedule(_) => AutomationKind::Schedule,
                StoredSuspendedConfiguration::Wake(_) => AutomationKind::Wake,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredSuspendedDefinition {
    pub legacy_timing: TimingRequest,
    pub configuration: StoredSuspendedConfiguration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum StoredSuspendedConfiguration {
    Schedule(SuspendedScheduleConfiguration),
    Wake(StoredSuspendedWake),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredSuspendedWake {
    pub target: SessionRef,
    pub origin: FiringMessageOrigin,
    pub delivery: MessageDelivery,
    #[serde(deserialize_with = "Option::deserialize")]
    pub generation_guard: Option<CodexGeneration>,
    pub content: ContentReference,
    pub prior_state: collaboration_protocol::WakeState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredScheduleDefinition {
    pub definition: ScheduleDefinition,
    pub timing: AutomationTiming,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredWakeDefinition {
    pub target: SessionRef,
    pub origin: FiringMessageOrigin,
    pub delivery: MessageDelivery,
    #[serde(deserialize_with = "Option::deserialize")]
    pub generation_guard: Option<CodexGeneration>,
    pub content: ContentReference,
    pub timing_request: TimingRequest,
    pub timing: AutomationTiming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CatalogState {
    Suspended,
    Enabled,
    Disabled,
    Deleted,
    Active,
    Paused,
    Cancelled,
    Expired,
    Finished,
    Replaced,
}
impl CatalogState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suspended => "suspended",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Deleted => "deleted",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Finished => "finished",
            Self::Replaced => "replaced",
        }
    }
    pub fn owns_wake_slot(self) -> bool {
        matches!(self, Self::Active | Self::Paused | Self::Suspended)
    }
}

pub(crate) struct AutomationRecord {
    pub id: AutomationId,
    pub revision: ChangeId,
    pub definition: StoredAutomationDefinition,
    pub state: CatalogState,
    pub creator: BoundAutomationCreator,
    pub next_due: Option<GridTime>,
    pub header: AutomationRecordHeader,
    pub expires_at: Option<ObservationTimestamp>,
    pub progress: AutomationProgress,
}

#[derive(Default)]
pub(crate) struct AutomationProgress {
    pub admitted_through: Option<GridTime>,
    pub occupying: Option<FiringId>,
    pub waiting: Option<FiringId>,
    pub external: Option<collaboration_protocol::RunId>,
}

impl AutomationRecord {
    pub fn wake_expiry(&self) -> AutomationStoreResult<&ObservationTimestamp> {
        self.expires_at
            .as_ref()
            .ok_or(AutomationStoreError::InvalidRecord("expires_at_ms"))
    }
}

pub(crate) fn timestamp_millis(value: &ObservationTimestamp) -> AutomationStoreResult<i64> {
    crate::automation_store_error::observation_millis(value)
}

pub(crate) fn validate_timing(
    request: &TimingRequest,
    timing: &AutomationTiming,
    kind: AutomationKind,
) -> AutomationStoreResult<()> {
    // Reconstruct from just before the chosen phase: a default first due remains
    // stable across edits, restarts and a later wall clock.
    let before = timing
        .first_due()
        .unix_millis()
        .checked_sub(1)
        .ok_or(collaboration_protocol::AutomationValidationError::TimeOutOfRange)?;
    let expected = AutomationTiming::resolve(request, kind, before)?;
    if expected != *timing {
        return Err(AutomationStoreError::InvalidRecord("definition.timing"));
    }
    Ok(())
}

pub(crate) fn wake_slot(target: &SessionRef) -> AutomationStoreResult<String> {
    serde_json::to_string(target).map_err(|_| AutomationStoreError::InvalidRecord("wake.target"))
}
