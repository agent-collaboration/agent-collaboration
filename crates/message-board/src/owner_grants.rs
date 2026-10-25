//! Relayed owner grants preserve provenance without claiming authentication.
use crate::{FeedCursor, OwnerGrantId, ProjectId, RecordedActor, RoleAssignmentId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GrantKind {
    ReplaceLead {
        project: ProjectId,
        new_holder: RecordedActor,
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OwnerWordsProvenance {
    Relayed { by: RecordedActor },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerGrant {
    pub envelope: crate::RecordEnvelope,
    pub id: OwnerGrantId,
    pub grant: GrantKind,
    pub provenance: OwnerWordsProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StandDownAcknowledgement {
    pub envelope: crate::RecordEnvelope,
    pub assignment: RoleAssignmentId,
    pub read_through: FeedCursor,
}
