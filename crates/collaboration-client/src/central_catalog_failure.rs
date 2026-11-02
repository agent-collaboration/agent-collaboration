//! The service's catalog failure envelope retains the caller identity and central's typed refusal.
use collaboration_protocol::{AutomationRefusal, OperationId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralCatalogFailure {
    #[error("central catalog unavailable; no mutation was dispatched")]
    CentralUnreachable {
        message: String,
        #[serde(deserialize_with = "Option::deserialize")]
        operation_id: Option<OperationId>,
    },
    #[error("central outcome unknown; inspect or replay the same operation ID")]
    OutcomeUnknown {
        operation_id: OperationId,
        message: String,
    },
    #[error("central catalog refused the request: {error:?}")]
    Refused {
        message: String,
        #[serde(deserialize_with = "Option::deserialize")]
        operation_id: Option<OperationId>,
        error: Box<AutomationRefusal>,
    },
}

impl CentralCatalogFailure {
    #[must_use]
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Self::CentralUnreachable { operation_id, .. } | Self::Refused { operation_id, .. } => {
                operation_id.as_ref()
            }
            Self::OutcomeUnknown { operation_id, .. } => Some(operation_id),
        }
    }
}
