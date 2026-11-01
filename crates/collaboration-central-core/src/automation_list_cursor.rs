//! List continuation names its collection and, for child rows, its parent.
use crate::automation_store_error::{AutomationStoreError, AutomationStoreResult};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::{
    AutomationId, AutomationRequestConstraint, ContentId, FiringId, InstructionId,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AutomationListCursor {
    Instructions {
        after: InstructionId,
    },
    Catalog {
        after: AutomationId,
    },
    Revisions {
        instruction_id: InstructionId,
        after: ContentId,
    },
    Firings {
        automation_id: AutomationId,
        after: FiringId,
    },
}

impl AutomationListCursor {
    pub(crate) fn decode(cursor: &str) -> AutomationStoreResult<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| invalid_cursor())?;
        serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())
    }

    pub(crate) fn encode(&self) -> AutomationStoreResult<String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| AutomationStoreError::InvalidRecord("list_cursor"))?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
}

pub(crate) fn invalid_cursor() -> AutomationStoreError {
    AutomationStoreError::InvalidRequest {
        field: "cursor",
        reason: AutomationRequestConstraint::InvalidCursor,
    }
}
