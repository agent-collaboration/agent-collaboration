use crate::{
    ActorIdentityId, AreaId, BacklogDescription, BacklogItemId, ChangeId, ChangePosition,
    DeliverableId, DeliverableRoleId, DependencyId, Description, EndpointId, InitiativeId, IssueId,
    IssueTitle, ItemRef, MachineId, PersonId, PostId, PostText, ProjectId, ProjectRoleId,
    ReadMarkId, ResourceName, SessionId, ThreadId, ThreadMemberId, TrailTag, TrailText,
    WorkLogEntryId, WorkspaceId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Timestamp = DateTime<Utc>;

macro_rules! closed_states {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum $name { $($variant),+ }
    };
}
closed_states!(WorkStatus {
    Backlog,
    Open,
    OnHold,
    Closed
});
closed_states!(DeliverableStatus { Open, Closed });
closed_states!(BacklogStatus { Open, Closed });
closed_states!(WorkspaceState { Active, Archived });
closed_states!(AreaState { Open, Archived });
closed_states!(ThreadState { Open, Resolved });
closed_states!(ProjectRole { Lead, Advisor });
closed_states!(DeliverableRole {
    Implementer,
    Verifier,
    Reviewer
});
closed_states!(MemberRole {
    Participant,
    Observer
});
closed_states!(NotificationSetting { On, Off });

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "role",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RoleName {
    Project(ProjectRole),
    Deliverable(DeliverableRole),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AssignmentState {
    Active,
    Ended { at: Timestamp },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MembershipState {
    Joined,
    Left { at: Timestamp },
}

macro_rules! records {
    ($($name:ident { $($field:ident: $value:ty),* $(,)? }),+ $(,)?) => {$ (
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(pub $field: $value),* }
    )+};
}
records! {
    Workspace { workspace_id: WorkspaceId, name: ResourceName, state: WorkspaceState },
    Area { area_id: AreaId, workspace_id: WorkspaceId, name: ResourceName, state: AreaState },
    Project { project_id: ProjectId, area_id: AreaId, title: ResourceName, status: WorkStatus, lead: ActorIdentityId },
    Initiative { initiative_id: InitiativeId, name: ResourceName, description: Description, status: WorkStatus },
    ActorIdentity { actor_identity_id: ActorIdentityId, identity: Identity },
    SessionAddress { machine_id: MachineId, endpoint: EndpointId, session_id: SessionId },
    ProjectRoleAssignment { project_role_id: ProjectRoleId, project_id: ProjectId, role: ProjectRole, holder: ActorIdentityId, granted_at: Timestamp, state: AssignmentState },
    Deliverable { deliverable_id: DeliverableId, project_id: ProjectId, title: ResourceName },
    DeliverableRoleAssignment { deliverable_role_id: DeliverableRoleId, deliverable_id: DeliverableId, role: DeliverableRole, holder: ActorIdentityId, granted_at: Timestamp, state: AssignmentState },
    DeliverableDependency { dependency_id: DependencyId, blocking: DeliverableId, blocked: DeliverableId },
    Issue { issue_id: IssueId, deliverable_id: DeliverableId, title: IssueTitle, status: WorkStatus },
    Thread { thread_id: ThreadId, area_id: AreaId, title: ResourceName, creator: ActorIdentityId, state: ThreadState },
    ThreadMember { thread_member_id: ThreadMemberId, thread_id: ThreadId, actor_identity_id: ActorIdentityId, role: MemberRole, notifications: NotificationSetting, state: MembershipState },
    Post { post_id: PostId, thread_id: ThreadId, author: ActorIdentityId, body: PostText, addressees: Vec<Addressee> },
    WorkLogEntry { entry_id: WorkLogEntryId, project_id: ProjectId, author: ActorIdentityId, body: WorkLogBody, tags: Vec<TrailTag> },
    BacklogItem { item_id: BacklogItemId, area_id: AreaId, title: ResourceName, description: BacklogDescription, creator: ActorIdentityId, status: BacklogStatus, closing_note: Option<String> },
    Change { change_id: ChangeId, position: ChangePosition, item: ItemRef, actor: ActorIdentityId, event: ChangeEvent },
    ReadMark { read_mark_id: ReadMarkId, actor_identity_id: ActorIdentityId, scope: ReadScope, through_position: ChangePosition }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Identity {
    AgentSession {
        session: SessionAddress,
    },
    RouterComponent {
        machine_id: MachineId,
        component: ResourceName,
    },
    Person {
        person_id: PersonId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkLogBody {
    Visible(TrailText),
    Redacted,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Addressee {
    Actor {
        actor_identity_id: ActorIdentityId,
    },
    ProjectRole {
        project_id: ProjectId,
        role: ProjectRole,
    },
    DeliverableRole {
        deliverable_id: DeliverableId,
        role: DeliverableRole,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReadScope {
    Inbox,
    Thread { thread_id: ThreadId },
}

/// A subject exists only for events about a holder/member; there is no nullable bag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChangeEvent {
    Created,
    Updated,
    Removed,
    Posted,
    Resolved,
    Redacted,
    StateChanged,
    Joined {
        member: ActorIdentityId,
    },
    Left {
        member: ActorIdentityId,
    },
    NotificationsOff {
        member: ActorIdentityId,
    },
    NotificationsOn {
        member: ActorIdentityId,
    },
    ProjectRoleGranted {
        holder: ActorIdentityId,
        role: ProjectRole,
    },
    ProjectRoleEnded {
        holder: ActorIdentityId,
        role: ProjectRole,
    },
    DeliverableRoleGranted {
        holder: ActorIdentityId,
        role: DeliverableRole,
    },
    DeliverableRoleEnded {
        holder: ActorIdentityId,
        role: DeliverableRole,
    },
}
