//! Read inputs identify a project/subject; they carry no persisted view state.
use crate::{Identity, ProjectId, RecordedActor, SessionRef, TrailSubject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectViewRequest {
    pub project_id: ProjectId,
    #[serde(default)]
    pub include_removed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContinuationCardRequest {
    pub project_id: ProjectId,
    pub actor: Identity,
    pub subject: TrailSubject,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionContextRequest {
    pub session: SessionRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerViewRequest {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeadLoadRequest {
    pub lead: RecordedActor,
}
