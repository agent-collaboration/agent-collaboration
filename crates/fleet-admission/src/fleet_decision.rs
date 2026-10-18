use chrono::{DateTime, Utc};
use message_board::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum FleetAdmissionError {
    #[error("the request identity has different canonical content")]
    ConflictingRequest,
    #[error("the fleet snapshot has invalid or incomplete data")]
    InvalidSnapshot,
    #[error("fleet request cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("fleet position or revision cannot advance: {0}")]
    Numeric(#[from] NumericValidationError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetIds {
    pub generated_record: RecordId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetWrite {
    Put(Box<Record>),
    RepositoryAttach {
        workspace: WorkspaceId,
        repository: RepositoryRef,
    },
    RepositoryDetach {
        workspace: WorkspaceId,
        repository: RepositoryRef,
    },
    Ledger(Box<RequestLedgerEntry>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetFeedEntry {
    pub position: FleetPosition,
    pub record: Record,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetDecision {
    pub decision: RequestDecision,
    pub writes: Vec<FleetWrite>,
    pub feed: Vec<FleetFeedEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "decision",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetRequestResult {
    Decided(RequestDecision),
    ConflictingRequest,
}

pub(crate) fn envelope(
    request: &crate::FleetRequest,
    id: RecordId,
    accepted_at: DateTime<Utc>,
    position: FleetPosition,
    board: Option<BoardId>,
) -> RecordEnvelope {
    RecordEnvelope {
        id,
        scope: RecordScope {
            workspace: request.workspace.clone(),
            board,
            project: None,
        },
        actor: request.actor.clone(),
        request: Some(request.request_id.clone()),
        position: RecordPosition::Fleet(position),
        router_time: accepted_at,
    }
}
