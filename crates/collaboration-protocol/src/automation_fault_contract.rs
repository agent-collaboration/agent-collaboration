//! A fault remains inspectable when its stored definition cannot be decoded.
use crate::{AutomationId, AutomationKind, ChangeId, ObservationTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationFault {
    pub error: AutomationRefusal,
    pub faulted_at: ObservationTimestamp,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FaultedAutomationSnapshot {
    pub automation_id: AutomationId,
    pub kind: AutomationKind,
    pub revision: ChangeId,
    pub fault: AutomationFault,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationRefusal {
    ImportConflict {
        automation_id: crate::AutomationId,
    },
    Validation {
        error: crate::AutomationValidationError,
    },
    OperationConflict {
        operation_id: crate::OperationId,
    },
    InstructionRevisionConflict {
        current_revision_id: crate::RevisionId,
    },
    ScheduleChangeConflict {
        current_change_id: crate::ChangeId,
    },
    ScheduleImportExists {
        schedule_id: crate::ScheduleId,
    },
    InstructionImportConflict {
        instruction_id: crate::InstructionId,
        schedule_ids: Vec<crate::ScheduleId>,
    },
    InvalidRecord {
        field: String,
    },
    InvalidRequest {
        field: String,
        reason: AutomationRequestConstraint,
    },
    WakeExpiryRequired,
    WakeLifecycleConflict {
        state: crate::WakeState,
    },
    WakeExists {
        creator: crate::WakeCreator,
    },
    TargetNeedsUpgrade {
        service_id: crate::UuidIdentity,
    },
    NotFound,
    OriginMismatch,
    DigestMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationRequestConstraint {
    MalformedShape,
    InvalidCursor,
    RequiredToken,
    AbsolutePathRequired,
    ExecutionModeFixed,
    UnpreparedDestination,
}
