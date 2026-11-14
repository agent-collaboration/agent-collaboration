//! Semantic caller inputs contain no central-minted entity identities.
use crate::{
    ActorIdentity, ActorIdentityId, Addressee, AreaId, BacklogItemId, ChangePosition,
    DeliverableId, DeliverableRole, DependencyId, Description, ExternalLink, Identity,
    InitiativeId, IssueId, IssueTitle, ItemRef, ItemView, LinkId, ListQuery, MemberRole,
    NotificationSetting, PostText, ProjectId, ProjectRole, ReadScope, ReferenceId, ReferenceSource,
    ReferenceTarget, RefusalReason, RequestId, ResourceName, SessionAddress, ThreadId, TrailTag,
    TrailText, WorkLogEntryId, WorkStatus, Workspace,
};
use serde::{Deserialize, Serialize};

/// Authenticated origin stamped by the Host, outside semantic tool input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Caller {
    Session(SessionAddress),
    Person {
        machine_id: crate::MachineId,
        person_id: crate::PersonId,
    },
}
impl Caller {
    #[must_use]
    pub fn machine_id(&self) -> &crate::MachineId {
        match self {
            Self::Session(session) => &session.machine_id,
            Self::Person { machine_id, .. } => machine_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ActorRef {
    Me,
    Actor(ActorIdentityId),
    Session(SessionAddress),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceOperation {
    CreateWorkspace {
        name: ResourceName,
    },
    CreateArea {
        name: ResourceName,
    },
    ArchiveArea {
        area_id: AreaId,
    },
    CreateProject {
        area_id: AreaId,
        title: ResourceName,
        lead: Option<ActorRef>,
    },
    CreateInitiative {
        name: ResourceName,
        description: Description,
    },
    UpdateInitiative {
        initiative_id: InitiativeId,
        name: ResourceName,
        description: Description,
    },
    CreateDeliverable {
        project_id: ProjectId,
        title: ResourceName,
    },
    CreateIssue {
        deliverable_id: DeliverableId,
        title: IssueTitle,
    },
    GrantProjectRole {
        project_id: ProjectId,
        role: ProjectRole,
        holder: ActorRef,
    },
    ReleaseProjectRole {
        project_id: ProjectId,
        role: ProjectRole,
    },
    GrantDeliverableRole {
        deliverable_id: DeliverableId,
        role: DeliverableRole,
        holder: ActorRef,
    },
    ReleaseDeliverableRole {
        deliverable_id: DeliverableId,
        role: DeliverableRole,
    },
    CreateThread {
        area_id: AreaId,
        title: ResourceName,
    },
    JoinThread {
        thread_id: ThreadId,
        role: MemberRole,
    },
    AddThreadMember {
        thread_id: ThreadId,
        actor: ActorIdentityId,
        role: MemberRole,
    },
    LeaveThread {
        thread_id: ThreadId,
    },
    SetNotifications {
        thread_id: ThreadId,
        setting: NotificationSetting,
    },
    ThreadPost {
        thread_id: ThreadId,
        body: PostText,
        addressees: Vec<Addressee>,
    },
    ResolveThread {
        thread_id: ThreadId,
    },
    AddDependency {
        blocking: DeliverableId,
        blocked: DeliverableId,
    },
    RemoveDependency {
        dependency_id: DependencyId,
    },
    SetIssueStatus {
        issue_id: IssueId,
        status: WorkStatus,
    },
    SetProjectStatus {
        project_id: ProjectId,
        status: WorkStatus,
    },
    SetInitiativeStatus {
        initiative_id: InitiativeId,
        status: WorkStatus,
    },
    AddBacklogItem {
        area_id: AreaId,
        title: ResourceName,
        description: Description,
    },
    UpdateBacklogItem {
        item_id: BacklogItemId,
        title: ResourceName,
        description: Description,
    },
    RemoveBacklogItem {
        item_id: BacklogItemId,
    },
    CloseBacklogItem {
        item_id: BacklogItemId,
        note: Description,
        link: Option<ItemRef>,
    },
    AddReference {
        source: ReferenceSource,
        target: ReferenceTarget,
        details: Option<serde_json::Value>,
    },
    RemoveReference {
        reference_id: ReferenceId,
    },
    AddLink {
        source: ReferenceSource,
        url: String,
        title: String,
        details: Option<serde_json::Value>,
    },
    RemoveLink {
        link_id: LinkId,
    },
    WorkLogAdd {
        project_id: ProjectId,
        body: TrailText,
        tags: Vec<TrailTag>,
    },
    WorkLogRedact {
        entry_id: WorkLogEntryId,
    },
    Show {
        item: ItemRef,
    },
    List {
        query: ListQuery,
        after: ChangePosition,
        limit: crate::PageLimit,
    },
    Inbox {
        after: Option<ChangePosition>,
        limit: crate::PageLimit,
    },
    MarkRead {
        scope: ReadScope,
        through: ChangePosition,
    },
}
impl WorkspaceOperation {
    #[must_use]
    pub const fn is_write(&self) -> bool {
        !matches!(
            self,
            Self::Show { .. } | Self::List { .. } | Self::Inbox { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceAnswer {
    WorkspaceCreated {
        workspace: Workspace,
    },
    Created {
        item: ItemRef,
        position: ChangePosition,
    },
    Updated {
        item: ItemRef,
        position: ChangePosition,
    },
    Removed {
        item: ItemRef,
        position: ChangePosition,
    },
    Unchanged {
        item: ItemRef,
    },
    Item {
        item: ItemView,
    },
    Page {
        items: Vec<ItemView>,
        next_after_position: Option<ChangePosition>,
    },
    References {
        items: Vec<crate::Reference>,
        next_after_position: Option<ChangePosition>,
    },
    Links {
        items: Vec<ExternalLink>,
        next_after_position: Option<ChangePosition>,
    },
    Inbox {
        entries: Vec<crate::InboxEntry>,
        next_after_position: Option<ChangePosition>,
        read_through: ChangePosition,
    },
    Marked {
        scope: ReadScope,
        through: ChangePosition,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceResult {
    Done(WorkspaceAnswer),
    Refused(RefusalReason),
    NotFound {
        workspace_id: crate::WorkspaceId,
        item: ItemRef,
    },
    Conflict,
    OutcomeUnknown,
    CentralUnavailable,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorView {
    pub actor_identity_id: ActorIdentityId,
    pub identity: Identity,
}
impl From<ActorIdentity> for ActorView {
    fn from(actor: ActorIdentity) -> Self {
        Self {
            actor_identity_id: actor.actor_identity_id,
            identity: actor.identity,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceReply {
    pub result: WorkspaceResult,
    pub actors: Vec<ActorView>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub request_id: Option<RequestId>,
    pub workspace_id: Option<crate::WorkspaceId>,
    pub operation: WorkspaceOperation,
}

#[cfg(test)]
#[path = "request_contract_test.rs"]
mod tests;
