use crate::{
    ActorIdentityId, Area, BacklogItem, Change, ChangePosition, Deliverable, DeliverableId,
    DeliverableStatus, Initiative, Issue, ItemRef, Post, Project, ReadScope, Thread, ThreadId,
    WorkLogEntry, WorkStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedItem {
    pub item: ItemRef,
    pub name: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blocker {
    pub item: ItemRef,
    pub title: String,
    pub status: BlockerStatus,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliverableView {
    pub deliverable: Deliverable,
    pub status: DeliverableStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "status",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BlockerStatus {
    Work(WorkStatus),
    Deliverable(DeliverableStatus),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RefusalReason {
    #[error("workspace is not allowed")]
    WorkspaceNotAllowed,
    #[error("item not found: {item:?}")]
    NotFound { item: ItemRef },
    #[error("only lead {lead} may grant roles")]
    NotLead {
        project: NamedItem,
        lead: ActorIdentityId,
    },
    #[error("project {project:?} must retain its lead")]
    LeadRequired { project: NamedItem },
    #[error("only creator {creator} may change this thread")]
    NotCreator { creator: ActorIdentityId },
    #[error("thread {thread_id} is resolved")]
    ThreadResolved { thread_id: ThreadId },
    #[error("actor is not a participant in {thread_id}")]
    NotParticipant { thread_id: ThreadId },
    #[error("actor is not a current member of {thread_id}")]
    NotMember { thread_id: ThreadId },
    #[error("dependency would cycle: {path:?}")]
    WouldCycle { path: Vec<DeliverableId> },
    #[error("dependency crosses projects")]
    CrossProject {
        blocking: DeliverableId,
        blocked: DeliverableId,
    },
    #[error("dependency already exists")]
    DuplicateDependency {
        blocking: DeliverableId,
        blocked: DeliverableId,
    },
    #[error("backlog description must not be empty")]
    EmptyDescription,
    #[error("target is outside this workspace: {item:?}")]
    TargetOutsideWorkspace { item: ItemRef },
    #[error("source kind is not allowed: {item:?}")]
    SourceKindNotAllowed { item: ItemRef },
    #[error("cannot close: children {blockers:?} remain open")]
    ChildrenNotClosed { blockers: Vec<Blocker> },
    #[error("cannot reopen: parent {parent:?} is closed")]
    ParentClosed { parent: Blocker },
    #[error("unknown actor {actor_identity_id}")]
    UnknownActor { actor_identity_id: ActorIdentityId },
    #[error("role addressee has no holder")]
    UnheldAddressee,
    #[error("invalid command input: {field}")]
    InvalidInput { field: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "item",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ItemView {
    Area(Area),
    Project(Project),
    Initiative(Initiative),
    Deliverable {
        view: DeliverableView,
        blocked_by: Vec<DeliverableId>,
    },
    Issue(Issue),
    Thread(Thread),
    Post(Post),
    WorkLogEntry(WorkLogEntry),
    BacklogItem(BacklogItem),
    Reference(crate::Reference),
    Link(crate::ExternalLink),
    Removed {
        item: ItemRef,
    },
}
impl ItemView {
    #[must_use]
    pub fn item_ref(&self) -> ItemRef {
        match self {
            Self::Area(row) => ItemRef::Area(row.area_id.clone()),
            Self::Project(row) => ItemRef::Project(row.project_id.clone()),
            Self::Initiative(row) => ItemRef::Initiative(row.initiative_id.clone()),
            Self::Deliverable { view, .. } => {
                ItemRef::Deliverable(view.deliverable.deliverable_id.clone())
            }
            Self::Issue(row) => ItemRef::Issue(row.issue_id.clone()),
            Self::Thread(row) => ItemRef::Thread(row.thread_id.clone()),
            Self::Post(row) => ItemRef::Post(row.post_id.clone()),
            Self::WorkLogEntry(row) => ItemRef::WorkLogEntry(row.entry_id.clone()),
            Self::BacklogItem(row) => ItemRef::BacklogItem(row.item_id.clone()),
            Self::Reference(row) => ItemRef::Reference(row.reference_id.clone()),
            Self::Link(row) => ItemRef::Link(row.link_id.clone()),
            Self::Removed { item } => item.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuleAnswer {
    Created {
        item: ItemRef,
    },
    Updated {
        item: ItemRef,
    },
    Removed {
        item: ItemRef,
    },
    Unchanged,
    Marked {
        scope: ReadScope,
        through: ChangePosition,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InboxEntry {
    pub position: ChangePosition,
    pub change: Change,
    pub item: ItemView,
}

#[must_use]
pub fn named_project(project: &Project) -> NamedItem {
    NamedItem {
        item: ItemRef::Project(project.project_id.clone()),
        name: project.title.as_str().to_owned(),
    }
}
