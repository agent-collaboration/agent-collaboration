use crate::{
    AreaId, BacklogItemId, DeliverableId, DeliverableRoleId, InitiativeId, IssueId, LinkId, PostId,
    ProjectId, ProjectRoleId, ReferenceId, ThreadId, ThreadMemberId, WorkLogEntryId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Workspace,
    Area,
    Project,
    Initiative,
    Deliverable,
    Issue,
    Thread,
    Post,
    WorkLogEntry,
    BacklogItem,
    Reference,
    Link,
    ThreadMember,
    ProjectRole,
    DeliverableRole,
}
impl ItemKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Area => "area",
            Self::Project => "project",
            Self::Initiative => "initiative",
            Self::Deliverable => "deliverable",
            Self::Issue => "issue",
            Self::Thread => "thread",
            Self::Post => "post",
            Self::WorkLogEntry => "workLogEntry",
            Self::BacklogItem => "backlogItem",
            Self::Reference => "reference",
            Self::Link => "link",
            Self::ThreadMember => "threadMember",
            Self::ProjectRole => "projectRole",
            Self::DeliverableRole => "deliverableRole",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ItemRef {
    Workspace(WorkspaceId),
    Area(AreaId),
    Project(ProjectId),
    Initiative(InitiativeId),
    Deliverable(DeliverableId),
    Issue(IssueId),
    Thread(ThreadId),
    Post(PostId),
    WorkLogEntry(WorkLogEntryId),
    BacklogItem(BacklogItemId),
    Reference(ReferenceId),
    Link(LinkId),
    ThreadMember(ThreadMemberId),
    ProjectRole(ProjectRoleId),
    DeliverableRole(DeliverableRoleId),
}
impl ItemRef {
    #[must_use]
    pub const fn kind(&self) -> ItemKind {
        match self {
            Self::Workspace(_) => ItemKind::Workspace,
            Self::Area(_) => ItemKind::Area,
            Self::Project(_) => ItemKind::Project,
            Self::Initiative(_) => ItemKind::Initiative,
            Self::Deliverable(_) => ItemKind::Deliverable,
            Self::Issue(_) => ItemKind::Issue,
            Self::Thread(_) => ItemKind::Thread,
            Self::Post(_) => ItemKind::Post,
            Self::WorkLogEntry(_) => ItemKind::WorkLogEntry,
            Self::BacklogItem(_) => ItemKind::BacklogItem,
            Self::Reference(_) => ItemKind::Reference,
            Self::Link(_) => ItemKind::Link,
            Self::ThreadMember(_) => ItemKind::ThreadMember,
            Self::ProjectRole(_) => ItemKind::ProjectRole,
            Self::DeliverableRole(_) => ItemKind::DeliverableRole,
        }
    }

    #[must_use]
    pub fn kind_and_id(&self) -> (&'static str, &str) {
        match self {
            Self::Workspace(id) => ("workspace", id.as_str()),
            Self::Area(id) => ("area", id.as_str()),
            Self::Project(id) => ("project", id.as_str()),
            Self::Initiative(id) => ("initiative", id.as_str()),
            Self::Deliverable(id) => ("deliverable", id.as_str()),
            Self::Issue(id) => ("issue", id.as_str()),
            Self::Thread(id) => ("thread", id.as_str()),
            Self::Post(id) => ("post", id.as_str()),
            Self::WorkLogEntry(id) => ("workLogEntry", id.as_str()),
            Self::BacklogItem(id) => ("backlogItem", id.as_str()),
            Self::Reference(id) => ("reference", id.as_str()),
            Self::Link(id) => ("link", id.as_str()),
            Self::ThreadMember(id) => ("threadMember", id.as_str()),
            Self::ProjectRole(id) => ("projectRole", id.as_str()),
            Self::DeliverableRole(id) => ("deliverableRole", id.as_str()),
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
    pub target: ReferenceTarget,
    pub details: Option<serde_json::Value>,
}

/// Relations, the workspace itself, memberships and role assignments have identity
/// for reads and paging, but cannot be reference targets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ItemRef", into = "ItemRef")]
pub struct ReferenceTarget(ItemRef);
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error(
    "a reference target cannot be a reference, a link, the workspace, a membership or a role assignment"
)]
pub struct InvalidReferenceTarget;
impl TryFrom<ItemRef> for ReferenceTarget {
    type Error = InvalidReferenceTarget;
    fn try_from(item: ItemRef) -> Result<Self, Self::Error> {
        if matches!(
            item,
            ItemRef::Reference(_)
                | ItemRef::Link(_)
                | ItemRef::Workspace(_)
                | ItemRef::ThreadMember(_)
                | ItemRef::ProjectRole(_)
                | ItemRef::DeliverableRole(_)
        ) {
            return Err(InvalidReferenceTarget);
        }
        Ok(Self(item))
    }
}
impl From<ReferenceTarget> for ItemRef {
    fn from(target: ReferenceTarget) -> Self {
        target.0
    }
}
impl ReferenceTarget {
    #[must_use]
    pub fn as_item_ref(&self) -> &ItemRef {
        &self.0
    }
    #[must_use]
    pub fn kind_and_id(&self) -> (&'static str, &str) {
        self.0.kind_and_id()
    }
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
