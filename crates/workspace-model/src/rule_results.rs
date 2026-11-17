use crate::{
    ActorIdentityId, Area, AreaId, BacklogItem, Change, ChangePosition, Deliverable, DeliverableId,
    DeliverableRoleAssignment, DeliverableStatus, Initiative, Issue, ItemRef, Post, Project,
    ProjectId, ProjectRoleAssignment, ReadScope, ResourceName, Thread, ThreadId, ThreadMember,
    ThreadState, WorkLogEntry, WorkStatus, Workspace,
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
    #[error("dependency {dependency_id} was not found")]
    DependencyNotFound { dependency_id: crate::DependencyId },
    #[error("machine is revoked, unknown, or its credential is invalid")]
    MachineRevoked,
    #[error("request id was reused by another caller or operation")]
    RequestIdReused,
    #[error("workspace is not allowed")]
    WorkspaceNotAllowed,
    #[error("workspace name is already in use: {name:?}")]
    WorkspaceNameInUse { name: crate::ResourceName },
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
    #[error("backlog item {item_id} is closed")]
    BacklogItemClosed { item_id: crate::BacklogItemId },
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
    Workspace(Workspace),
    Area(Area),
    Project(Project),
    Initiative(Initiative),
    Deliverable {
        view: DeliverableView,
        blocked_by: Vec<DeliverableId>,
    },
    Issue(Issue),
    /// The thread plus the caller's own membership, when the caller is a member.
    Thread {
        thread: Thread,
        me: Option<ThreadReaderView>,
    },
    ThreadMember(ThreadMember),
    ProjectRole(ProjectRoleAssignment),
    DeliverableRole(DeliverableRoleAssignment),
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
            Self::Workspace(row) => ItemRef::Workspace(row.workspace_id.clone()),
            Self::Area(row) => ItemRef::Area(row.area_id.clone()),
            Self::Project(row) => ItemRef::Project(row.project_id.clone()),
            Self::Initiative(row) => ItemRef::Initiative(row.initiative_id.clone()),
            Self::Deliverable { view, .. } => {
                ItemRef::Deliverable(view.deliverable.deliverable_id.clone())
            }
            Self::Issue(row) => ItemRef::Issue(row.issue_id.clone()),
            Self::Thread { thread, .. } => ItemRef::Thread(thread.thread_id.clone()),
            Self::ThreadMember(row) => ItemRef::ThreadMember(row.thread_member_id.clone()),
            Self::ProjectRole(row) => ItemRef::ProjectRole(row.project_role_id.clone()),
            Self::DeliverableRole(row) => ItemRef::DeliverableRole(row.deliverable_role_id.clone()),
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
/// The caller's own standing in a thread. Reading it moves no read mark.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadReaderView {
    pub role: crate::MemberRole,
    pub notifications: crate::NotificationSetting,
    pub read_through: Option<ChangePosition>,
}
/// One workspace's inbox page covers an area's threads or a project's.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InboxScope {
    Area { area_id: AreaId },
    Project { project_id: ProjectId },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InboxReadMode {
    /// Items after the reader's inbox read mark, oldest first.
    Unread,
    /// The newest items, newest first, regardless of the read mark.
    Latest,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListOrder {
    OldestFirst,
    NewestFirst,
}
/// Where a list page continues: a position and the direction it continues in.
/// Oldest-first continues strictly after the position, newest-first strictly
/// before it; without a position the page starts from that direction's end.
/// Callers start with [`ListCursor::start`] and otherwise hand back the
/// `next_cursor` of the previous page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListCursor {
    order: ListOrder,
    position: Option<ChangePosition>,
}
impl ListCursor {
    #[must_use]
    pub const fn start(order: ListOrder) -> Self {
        Self {
            order,
            position: None,
        }
    }
    /// The cursor that continues past `position` in the same direction.
    #[must_use]
    pub const fn continue_past(self, position: ChangePosition) -> Self {
        Self {
            order: self.order,
            position: Some(position),
        }
    }
    #[must_use]
    pub const fn order(self) -> ListOrder {
        self.order
    }
    #[must_use]
    pub const fn position(self) -> Option<ChangePosition> {
        self.position
    }
}
/// One inbox item, with the names a reader needs to place it. Reading moves no
/// read mark; `acknowledge` is what MarkRead takes for it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InboxItem {
    PostCreated {
        position: ChangePosition,
        acknowledge: ReadScope,
        post: Post,
        thread_title: ResourceName,
        area_name: ResourceName,
        project_name: Option<ResourceName>,
    },
    ThreadStateChanged {
        position: ChangePosition,
        acknowledge: ReadScope,
        thread_id: ThreadId,
        actor: ActorIdentityId,
        state: ThreadState,
        thread_title: ResourceName,
        area_name: ResourceName,
        project_name: Option<ResourceName>,
    },
    /// A role, status or work-log change inside a Project-scope inbox.
    ProjectChanged {
        position: ChangePosition,
        acknowledge: ReadScope,
        change: Change,
        project_name: ResourceName,
        area_name: ResourceName,
    },
}

#[must_use]
pub fn named_project(project: &Project) -> NamedItem {
    NamedItem {
        item: ItemRef::Project(project.project_id.clone()),
        name: project.title.as_str().to_owned(),
    }
}
