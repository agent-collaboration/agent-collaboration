//! Public fleet writes carry declared identities; the Router adds recorded provenance.
use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdrCreateRequest {
    pub request_id: RequestId,
    pub protocol_version: ProtocolVersion,
    pub workspace_id: WorkspaceId,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub adr_id: AdrId,
    pub title: ResourceName,
    pub text: MarkdownText,
    pub supersedes: Option<AdrId>,
    pub made_in: Option<RecordRef>,
    pub repository_files: Vec<ArtifactRef>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdrTransitionRequest {
    pub request_id: RequestId,
    pub protocol_version: ProtocolVersion,
    pub workspace_id: WorkspaceId,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub adr_id: AdrId,
    pub transition: AdrTransition,
    pub expected_revision: SubjectRevision,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessRegisterRequest {
    pub request_id: RequestId,
    pub protocol_version: ProtocolVersion,
    pub workspace_id: WorkspaceId,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub name: ResourceName,
    pub version: ResourceName,
    pub source: String,
}
