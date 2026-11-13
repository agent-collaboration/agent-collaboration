use crate::{
    AreaId, BacklogItemId, DeliverableId, InitiativeId, IssueId, LinkId, PostId, ProjectId,
    ReferenceId, ThreadId, WorkLogEntryId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ItemRef {
    Area(AreaId),
    Project(ProjectId),
    Initiative(InitiativeId),
    Deliverable(DeliverableId),
    Issue(IssueId),
    Thread(ThreadId),
    Post(PostId),
    WorkLogEntry(WorkLogEntryId),
    BacklogItem(BacklogItemId),
}
impl ItemRef {
    #[must_use]
    pub fn kind_and_id(&self) -> (&'static str, &str) {
        match self {
            Self::Area(id) => ("area", id.as_str()),
            Self::Project(id) => ("project", id.as_str()),
            Self::Initiative(id) => ("initiative", id.as_str()),
            Self::Deliverable(id) => ("deliverable", id.as_str()),
            Self::Issue(id) => ("issue", id.as_str()),
            Self::Thread(id) => ("thread", id.as_str()),
            Self::Post(id) => ("post", id.as_str()),
            Self::WorkLogEntry(id) => ("workLogEntry", id.as_str()),
            Self::BacklogItem(id) => ("backlogItem", id.as_str()),
        }
    }
}

/// Issues and areas cannot own references or external links.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReferenceSource {
    Initiative(InitiativeId),
    Deliverable(DeliverableId),
    Thread(ThreadId),
    Post(PostId),
    WorkLogEntry(WorkLogEntryId),
    BacklogItem(BacklogItemId),
}
impl From<ReferenceSource> for ItemRef {
    fn from(value: ReferenceSource) -> Self {
        match value {
            ReferenceSource::Initiative(id) => Self::Initiative(id),
            ReferenceSource::Deliverable(id) => Self::Deliverable(id),
            ReferenceSource::Thread(id) => Self::Thread(id),
            ReferenceSource::Post(id) => Self::Post(id),
            ReferenceSource::WorkLogEntry(id) => Self::WorkLogEntry(id),
            ReferenceSource::BacklogItem(id) => Self::BacklogItem(id),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reference {
    pub reference_id: ReferenceId,
    pub source: ReferenceSource,
    pub target: ItemRef,
    pub details: Option<serde_json::Value>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalLink {
    pub link_id: LinkId,
    pub source: ReferenceSource,
    pub url: String,
    pub title: String,
    pub details: Option<serde_json::Value>,
}
