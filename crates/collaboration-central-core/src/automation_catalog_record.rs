//! Stored definitions own configuration; their separate row owns timing progress.
use crate::automation_store_error::{AutomationStoreError, AutomationStoreResult};
use collaboration_protocol::{
    AutomationId, AutomationKind, AutomationTiming, ChangeId, CodexGeneration, ContentReference,
    FiringMessageOrigin, GridTime, ImportedContinuity, MessageDelivery, ObservationTimestamp,
    ScheduleDefinition, SessionRef, TimingRequest, WakeCreator,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredScheduleDefinition {
    pub definition: ScheduleDefinition,
    pub timing: AutomationTiming,
    pub imported_continuity: ImportedContinuity,
    pub anchor_at: ObservationTimestamp,
    pub created_at: ObservationTimestamp,
    pub updated_at: ObservationTimestamp,
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
    pub anchor_at: ObservationTimestamp,
    pub expires_at: ObservationTimestamp,
    pub created_at: ObservationTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CatalogState {
    Enabled,
    Disabled,
    Deleted,
    Active,
    Paused,
    Cancelled,
    Expired,
    Finished,
}
impl CatalogState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Deleted => "deleted",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Finished => "finished",
        }
    }
    pub fn owns_wake_slot(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }
}

pub(crate) struct AutomationRecord {
    pub id: AutomationId,
    pub revision: ChangeId,
    pub definition: StoredAutomationDefinition,
    pub state: CatalogState,
    pub creator: BoundAutomationCreator,
    pub next_due: Option<GridTime>,
}

pub(crate) fn timestamp_millis(value: &ObservationTimestamp) -> AutomationStoreResult<i64> {
    String::from(value.clone())
        .parse::<chrono::DateTime<chrono::Utc>>()
        .map(|time| time.timestamp_millis())
        .map_err(|_| AutomationStoreError::InvalidRecord("timestamp"))
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
