use crate::{
    ActorIdentityId, Addressee, AreaId, BacklogItemId, ChangePosition, DeliverableId,
    DeliverableRole, DeliverableRoleId, DependencyId, Description, ExternalLink, InitiativeId,
    IssueId, IssueTitle, ItemRef, LinkId, MemberRole, NotificationSetting, PostId, PostText,
    ProjectId, ProjectRole, ProjectRoleId, ReadScope, Reference, ReferenceId, ResourceName,
    ThreadId, ThreadMemberId, TrailTag, TrailText, WorkLogEntryId, WorkStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationCommand {
    CreateArea {
        area_id: AreaId,
        name: ResourceName,
    },
    ArchiveArea {
        area_id: AreaId,
    },
    CreateProject {
        project_id: ProjectId,
        area_id: AreaId,
        title: ResourceName,
        lead: ActorIdentityId,
        role_id: ProjectRoleId,
    },
    CreateInitiative {
        initiative_id: InitiativeId,
        name: ResourceName,
        description: Description,
    },
    UpdateInitiative {
        initiative_id: InitiativeId,
        name: ResourceName,
        description: Description,
    },
    CreateDeliverable {
        deliverable_id: DeliverableId,
        project_id: ProjectId,
        title: ResourceName,
    },
    CreateIssue {
        issue_id: IssueId,
        deliverable_id: DeliverableId,
        title: IssueTitle,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RoleCommand {
    GrantProject {
        assignment_id: ProjectRoleId,
        project_id: ProjectId,
        role: ProjectRole,
        holder: ActorIdentityId,
    },
    ReleaseProject {
        project_id: ProjectId,
        role: ProjectRole,
    },
    GrantDeliverable {
        assignment_id: DeliverableRoleId,
        deliverable_id: DeliverableId,
        role: DeliverableRole,
        holder: ActorIdentityId,
    },
    ReleaseDeliverable {
        deliverable_id: DeliverableId,
        role: DeliverableRole,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ThreadCommand {
    CreateThread {
        thread_id: ThreadId,
        member_id: ThreadMemberId,
        area_id: AreaId,
        title: ResourceName,
    },
    JoinThread {
        thread_id: ThreadId,
        member_id: ThreadMemberId,
    },
    AddThreadMember {
        thread_id: ThreadId,
        member_id: ThreadMemberId,
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
        post_id: PostId,
        thread_id: ThreadId,
        body: PostText,
        addressees: Vec<Addressee>,
    },
    ResolveThread {
        thread_id: ThreadId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BacklogCommand {
    Add {
        item_id: BacklogItemId,
        area_id: AreaId,
        title: ResourceName,
        description: Description,
    },
    Update {
        item_id: BacklogItemId,
        title: ResourceName,
        description: Description,
    },
    Remove {
        item_id: BacklogItemId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReferenceCommand {
    AddReference { reference: Reference },
    RemoveReference { reference_id: ReferenceId },
    AddLink { link: ExternalLink },
    RemoveLink { link_id: LinkId },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DependencyCommand {
    Add {
        dependency_id: DependencyId,
        blocking: DeliverableId,
        blocked: DeliverableId,
    },
    Remove {
        dependency_id: DependencyId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StatusCommand {
    SetIssue {
        issue_id: IssueId,
        status: WorkStatus,
    },
    SetProject {
        project_id: ProjectId,
        status: WorkStatus,
    },
    SetInitiative {
        initiative_id: InitiativeId,
        status: WorkStatus,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkLogCommand {
    Add {
        entry_id: WorkLogEntryId,
        project_id: ProjectId,
        body: TrailText,
        tags: Vec<TrailTag>,
    },
    Redact {
        entry_id: WorkLogEntryId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ListQuery {
    Areas,
    Projects {
        area_id: AreaId,
    },
    Threads {
        area_id: AreaId,
    },
    Posts {
        thread_id: ThreadId,
    },
    Deliverables {
        project_id: ProjectId,
    },
    Issues {
        deliverable_id: DeliverableId,
    },
    WorkLog {
        project_id: ProjectId,
    },
    Backlog {
        area_id: AreaId,
    },
    ReferencesTo {
        item: ItemRef,
    },
    ReferencesFrom {
        item: ItemRef,
    },
    Links {
        item: ItemRef,
    },
    SearchPosts {
        area_id: Option<AreaId>,
        thread_id: Option<ThreadId>,
        text: crate::SearchQuery,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceCommand {
    Organization(OrganizationCommand),
    Role(RoleCommand),
    Thread(ThreadCommand),
    Backlog(BacklogCommand),
    Reference(ReferenceCommand),
    Dependency(DependencyCommand),
    Status(StatusCommand),
    WorkLog(WorkLogCommand),
    Show(ItemRef),
    List(ListQuery),
    MarkRead {
        scope: ReadScope,
        through: ChangePosition,
    },
}
