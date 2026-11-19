//! Semantic caller inputs contain no central-minted entity identities.
use crate::{
    ActorIdentity, ActorIdentityId, Addressee, AreaId, BacklogItemId, ChangePosition,
    DeliverableId, DeliverableRole, DependencyId, Description, ExternalLink, Identity,
    InitiativeId, IssueId, IssueTitle, ItemRef, ItemView, LinkId, ListQuery, MemberRole,
    NotificationSetting, PostText, ProjectId, ProjectRole, ReadScope, ReferenceId, ReferenceSource,
    ReferenceTarget, RefusalReason, RequestId, ResourceName, ThreadId, TrailTag, TrailText,
    WorkLogEntryId, WorkStatus, Workspace,
};
use serde::{Deserialize, Serialize};
use workspace_ids::SessionRef;

/// A session's thread notices share one cadence across all workspaces.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum NoticeCadence {
    #[default]
    #[serde(rename = "m15")]
    M15,
    #[serde(rename = "m25")]
    M25,
    #[serde(rename = "m55")]
    M55,
}
impl NoticeCadence {
    #[must_use]
    pub const fn seconds(self) -> u32 {
        match self {
            Self::M15 => 900,
            Self::M25 => 1500,
            Self::M55 => 3300,
        }
    }
}

/// Authenticated origin stamped by the Host, outside semantic tool input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Caller {
    Session(SessionRef),
    Person {
        machine_id: workspace_ids::MachineId,
        person_id: crate::PersonId,
    },
}
impl Caller {
    #[must_use]
    pub fn machine_id(&self) -> &workspace_ids::MachineId {
        match self {
            Self::Session(session) => &session.machine_id,
            Self::Person { machine_id, .. } => machine_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ActorRef {
    Me,
    Actor(ActorIdentityId),
    Session(SessionRef),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Targets the request's workspace. The canonical name stays unique per organization.
    UpdateWorkspace {
        name: ResourceName,
        description: Description,
    },
    /// Targets the request's workspace.
    ArchiveWorkspace,
    /// The description is required and must not be blank.
    CreateArea {
        name: ResourceName,
        description: Description,
    },
    ArchiveArea {
        area_id: AreaId,
    },
    /// An omitted description stays as it is; a given one must not be blank.
    UpdateArea {
        area_id: AreaId,
        name: ResourceName,
        description: Option<Description>,
    },
    /// The description is required and must not be blank.
    CreateProject {
        area_id: AreaId,
        title: ResourceName,
        description: Description,
        lead: Option<ActorRef>,
    },
    /// An omitted description stays as it is; a given one must not be blank.
    UpdateProject {
        project_id: ProjectId,
        title: ResourceName,
        description: Option<Description>,
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
    UpdateIssue {
        issue_id: IssueId,
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
    /// The thread, its creator's membership and its first post are one change set.
    CreateThread {
        area_id: AreaId,
        title: ResourceName,
        body: PostText,
        addressees: Vec<Addressee>,
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
    /// An optional closing post commits atomically with the resolution.
    ResolveThread {
        thread_id: ThreadId,
        closing_post: Option<PostText>,
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
        cursor: crate::ListCursor,
        limit: crate::PageLimit,
    },
    Inbox {
        scope: crate::InboxScope,
        read_mode: crate::InboxReadMode,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceAnswer {
    MessageStatus {
        status: crate::central_mail::MessageStatus,
    },
    WorkspaceCreated {
        workspace: Workspace,
    },
    /// The workspace after an update or archive.
    WorkspaceUpdated {
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
        next_cursor: Option<crate::ListCursor>,
    },
    References {
        items: Vec<crate::Reference>,
        next_cursor: Option<crate::ListCursor>,
    },
    Links {
        items: Vec<ExternalLink>,
        next_cursor: Option<crate::ListCursor>,
    },
    Inbox {
        scope: crate::InboxScope,
        read_mode: crate::InboxReadMode,
        order: crate::ListOrder,
        items: Vec<crate::InboxItem>,
        /// The workspace's latest change position when the page was read.
        high_water: ChangePosition,
        /// Projects in scope whose items could not be read for this page.
        unavailable_projects: Vec<ProjectId>,
        next_after_position: Option<ChangePosition>,
    },
    Marked {
        scope: ReadScope,
        through: ChangePosition,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceReply {
    pub result: WorkspaceResult,
    pub actors: Vec<ActorView>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub request_id: Option<RequestId>,
    pub workspace_id: Option<crate::WorkspaceId>,
    pub operation: WorkspaceOperation,
}

#[cfg(test)]
#[path = "request_contract_test.rs"]
mod tests;
