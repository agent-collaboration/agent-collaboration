//! Project description, revision and closing state.
use crate::{FeedCursor, ProjectId, SubjectRevision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectState {
    pub envelope: crate::RecordEnvelope,
    pub project_id: ProjectId,
    pub description: crate::Description,
    pub revision: SubjectRevision,
    pub closed_at: Option<FeedCursor>,
}
