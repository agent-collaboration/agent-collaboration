use crate::{
    ActorIdentityId, Addressee, AreaId, BacklogItemId, ChangeId, ChangePosition, DeliverableId,
    DeliverableRole, DeliverableRoleId, DependencyId, Description, ExternalLink, InitiativeId,
    IssueId, IssueTitle, ItemRef, LinkId, MemberRole, NotificationSetting, PostId, PostText,
    ProjectId, ProjectRole, ProjectRoleId, ReadScope, Reference, ReferenceId, RequiredDescription,
    ResourceName, ThreadId, ThreadMemberId, TrailTag, TrailText, WorkLogEntryId, WorkStatus,
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
        description: RequiredDescription,
    },
    ArchiveArea {
        area_id: AreaId,
    },
    UpdateArea {
        area_id: AreaId,
        name: ResourceName,
        description: Option<RequiredDescription>,
    },
    CreateProject {
        project_id: ProjectId,
        area_id: AreaId,
        title: ResourceName,
        description: RequiredDescription,
        lead: ActorIdentityId,
        role_id: ProjectRoleId,
    },
    UpdateProject {
        project_id: ProjectId,
        title: ResourceName,
        description: Option<RequiredDescription>,
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
    UpdateIssue {
        issue_id: IssueId,
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
    /// The thread, the creator's membership and the first post are one step.
    CreateThread {
        thread_id: ThreadId,
        member_id: ThreadMemberId,
        area_id: AreaId,
        title: ResourceName,
        first_post: ThreadPostDraft,
    },
    JoinThread {
        thread_id: ThreadId,
        member_id: ThreadMemberId,
        role: MemberRole,
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
    /// A closing post is written in the same step, before the thread resolves.
    ResolveThread {
        thread_id: ThreadId,
        closing_post: Option<ThreadPostDraft>,
    },
}

/// A post written as part of another thread step, with its minted id.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadPostDraft {
    pub post_id: PostId,
    /// The post's own change row, so inbox and notices see it as a post.
    pub change_id: ChangeId,
    pub body: PostText,
    pub addressees: Vec<Addressee>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BacklogCommand {
    Close {
        item_id: BacklogItemId,
        note: Description,
        link: Option<BacklogClosingReference>,
    },
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

/// An optional close reference carries its own creation change for reference paging.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BacklogClosingReference {
    pub reference: Reference,
    pub change_id: ChangeId,
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ListQuery {
    /// Delivery status for a message the caller sent.
    MessageStatus {
        message_id: crate::MessageId,
    },
    /// The organization's workspaces; sent without a workspace id.
    Workspaces {
        include_archived: bool,
    },
    /// Workspaces whose name or description contains the text; sent without a
    /// workspace id.
    SearchWorkspaces {
        text: crate::SearchQuery,
        include_archived: bool,
    },
    Areas {
        include_archived: bool,
    },
    Initiatives,
    Projects {
        area_id: AreaId,
    },
    /// Threads in one area, or in the whole workspace when `area_id` is absent.
    /// `followed_only` keeps threads the caller is a current member of.
    Threads {
        area_id: Option<AreaId>,
        followed_only: bool,
    },
    ThreadMembers {
        thread_id: ThreadId,
        include_left: bool,
    },
    Posts {
        scope: PostScope,
    },
    Deliverables {
        project_id: ProjectId,
    },
    Issues {
        deliverable_id: DeliverableId,
    },
    /// Every issue under the project's deliverables.
    ProjectIssues {
        project_id: ProjectId,
    },
    ProjectRoles {
        project_id: ProjectId,
        include_ended: bool,
    },
    DeliverableRoles {
        deliverable_id: DeliverableId,
        include_ended: bool,
    },
    /// A project's work log. With tags, only entries that carry every listed
    /// tag; with none, every entry.
    WorkLog {
        project_id: ProjectId,
        tags: Vec<TrailTag>,
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
        scope: PostScope,
        text: crate::SearchQuery,
    },
    /// Areas, initiatives and projects whose name, title or description contains
    /// the text.
    SearchItems {
        text: crate::SearchQuery,
        item_kind: SearchableKind,
        include_archived: bool,
    },
}

/// Where a post read looks, inside one workspace. Threads belong to areas, so
/// there is no project scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PostScope {
    Workspace,
    Area { area_id: AreaId },
    Thread { thread_id: ThreadId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchableKind {
    All,
    Area,
    Initiative,
    Project,
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
