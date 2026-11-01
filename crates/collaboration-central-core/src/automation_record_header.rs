//! Row-owned metadata survives replacement of an unreadable executable definition.
use crate::automation_store_error::{
    AutomationStoreError, AutomationStoreResult, observation_millis, timestamp, timestamp_millis,
};
use collaboration_protocol::{
    AutomationKind, AutomationRequestConstraint, ExecutionDestination, ImportedContinuity,
    ObservationTimestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ScheduleExecutionMode {
    OwnedThread,
    FreshEachRun,
}
impl ScheduleExecutionMode {
    pub fn for_destination(destination: &ExecutionDestination) -> Self {
        match destination {
            ExecutionDestination::Unprepared | ExecutionDestination::OwnedThread { .. } => {
                Self::OwnedThread
            }
            ExecutionDestination::FreshEachRunUnprepared
            | ExecutionDestination::FreshEachRun { .. } => Self::FreshEachRun,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnedThread => "ownedThread",
            Self::FreshEachRun => "freshEachRun",
        }
    }
    pub fn require_destination(
        self,
        destination: &ExecutionDestination,
    ) -> AutomationStoreResult<()> {
        if self != Self::for_destination(destination) {
            return Err(AutomationStoreError::InvalidRequest {
                field: "destination",
                reason: AutomationRequestConstraint::ExecutionModeFixed,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RecordTimestamps {
    pub anchor_at: ObservationTimestamp,
    pub created_at: ObservationTimestamp,
    pub updated_at: ObservationTimestamp,
}
impl RecordTimestamps {
    fn created(now_ms: i64) -> AutomationStoreResult<Self> {
        let now = timestamp(now_ms)?;
        Ok(Self {
            anchor_at: now.clone(),
            created_at: now.clone(),
            updated_at: now,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AutomationRecordHeader {
    Schedule {
        times: RecordTimestamps,
        imported_continuity: Option<ImportedContinuity>,
        execution_mode: ScheduleExecutionMode,
    },
    Wake {
        times: RecordTimestamps,
    },
}

pub(crate) struct StoredHeaderFields {
    pub anchor_at_ms: Option<String>,
    pub created_at_ms: Option<String>,
    pub updated_at_ms: Option<String>,
    pub imported_continuity_json: Option<String>,
    pub execution_mode: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct HeaderColumnValues {
    pub anchor_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub imported_continuity_json: Option<String>,
    pub execution_mode: Option<&'static str>,
}

impl AutomationRecordHeader {
    pub fn schedule(
        destination: &ExecutionDestination,
        now_ms: i64,
    ) -> AutomationStoreResult<Self> {
        Ok(Self::Schedule {
            times: RecordTimestamps::created(now_ms)?,
            imported_continuity: None,
            execution_mode: ScheduleExecutionMode::for_destination(destination),
        })
    }
    pub fn wake(now_ms: i64) -> AutomationStoreResult<Self> {
        Ok(Self::Wake {
            times: RecordTimestamps::created(now_ms)?,
        })
    }
    pub fn times(&self) -> &RecordTimestamps {
        match self {
            Self::Schedule { times, .. } | Self::Wake { times } => times,
        }
    }
    pub fn set_updated(&mut self, now_ms: i64) -> AutomationStoreResult<()> {
        let times = match self {
            Self::Schedule { times, .. } | Self::Wake { times } => times,
        };
        times.updated_at = timestamp(now_ms)?;
        Ok(())
    }
    pub fn decode(kind: AutomationKind, fields: StoredHeaderFields) -> AutomationStoreResult<Self> {
        let times = RecordTimestamps {
            anchor_at: timestamp(timestamp_millis(fields.anchor_at_ms, "anchor_at_ms")?)?,
            created_at: timestamp(timestamp_millis(fields.created_at_ms, "created_at_ms")?)?,
            updated_at: timestamp(timestamp_millis(fields.updated_at_ms, "updated_at_ms")?)?,
        };
        match kind {
            AutomationKind::Wake => {
                if fields.imported_continuity_json.is_some() || fields.execution_mode.is_some() {
                    return Err(AutomationStoreError::InvalidRecord("wake.record_header"));
                }
                Ok(Self::Wake { times })
            }
            AutomationKind::Schedule => {
                let execution_mode = match fields.execution_mode.as_deref() {
                    Some("ownedThread") => ScheduleExecutionMode::OwnedThread,
                    Some("freshEachRun") => ScheduleExecutionMode::FreshEachRun,
                    _ => return Err(AutomationStoreError::InvalidRecord("execution_mode")),
                };
                let imported_continuity = fields
                    .imported_continuity_json
                    .map(|raw| {
                        let value: serde_json::Value =
                            serde_json::from_str(&raw).map_err(|_| {
                                AutomationStoreError::InvalidRecord("imported_continuity_json")
                            })?;
                        crate::automation_request_decode::identity_limits(&value)?;
                        crate::automation_request_decode::counter_limits(&value)?;
                        serde_json::from_value(value).map_err(|_| {
                            AutomationStoreError::InvalidRecord("imported_continuity_json")
                        })
                    })
                    .transpose()?;
                Ok(Self::Schedule {
                    times,
                    imported_continuity,
                    execution_mode,
                })
            }
        }
    }
    pub fn columns(&self) -> AutomationStoreResult<HeaderColumnValues> {
        let times = self.times();
        let (continuity, mode) = match self {
            Self::Schedule {
                imported_continuity,
                execution_mode,
                ..
            } => (
                imported_continuity
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|_| AutomationStoreError::InvalidRecord("imported_continuity_json"))?,
                Some(execution_mode.as_str()),
            ),
            Self::Wake { .. } => (None, None),
        };
        Ok(HeaderColumnValues {
            anchor_at_ms: observation_millis(&times.anchor_at)?,
            created_at_ms: observation_millis(&times.created_at)?,
            updated_at_ms: observation_millis(&times.updated_at)?,
            imported_continuity_json: continuity,
            execution_mode: mode,
        })
    }
}
