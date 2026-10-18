//! An ADR's immutable Markdown content is separate from its lifecycle.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AdrId, ArtifactRef, MarkdownText, RecordRef, ResourceName, SubjectRevision, WorkspaceId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdrStatus {
    Proposed,
    Accepted,
    Superseded,
    Withdrawn,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Adr {
    pub envelope: crate::RecordEnvelope,
    pub adr_id: AdrId,
    pub workspace: WorkspaceId,
    pub title: ResourceName,
    pub text: MarkdownText,
    pub status: AdrStatus,
    pub supersedes: Option<AdrId>,
    pub made_in: Option<RecordRef>,
    pub repository_files: Vec<ArtifactRef>,
    pub revision: SubjectRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AdrTransition {
    Accept {},
    Withdraw {},
    Supersede { by: AdrId },
}
