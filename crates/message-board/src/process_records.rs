//! Project state pins one immutable process definition; guards consume recorded facts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRole, CanonicalHash, FeedCursor, Identity, MoveName, ProjectId, ResourceName,
    StatusName, SubjectRevision,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessDefinitionRef {
    pub name: ResourceName,
    pub version: ResourceName,
    pub source_hash: CanonicalHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuardRequirement {
    pub name: ResourceName,
    pub evaluator: Identity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessEdge {
    pub name: MoveName,
    pub from: StatusName,
    pub to: StatusName,
    pub required_role: AuthorityRole,
    pub guards: Vec<GuardRequirement>,
}

/// Exact registered source text is parsed into this document, never a caller-supplied graph.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessDefinitionSource {
    pub initial_status: StatusName,
    pub edges: Vec<ProcessEdge>,
}

impl ProcessDefinitionSource {
    pub fn parse(source: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessDefinition {
    pub envelope: crate::RecordEnvelope,
    pub reference: ProcessDefinitionRef,
    pub source: String,
    pub initial_status: StatusName,
    pub edges: Vec<ProcessEdge>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectState {
    pub envelope: crate::RecordEnvelope,
    pub project_id: ProjectId,
    pub status: Option<StatusName>,
    pub pinned_definition: Option<ProcessDefinitionRef>,
    pub revision: SubjectRevision,
    pub closed_at: Option<FeedCursor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GuardOutcome {
    Met,
    Unmet,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordedGuardResult {
    pub guard: ResourceName,
    pub evaluator: Identity,
    pub outcome: GuardOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MoveVerdict {
    Allowed {},
    Rejected { reasons: Vec<crate::RejectReason> },
}
