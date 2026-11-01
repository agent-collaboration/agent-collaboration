//! Read-only presence answers and owner snapshots retain their distinct time units.
use super::{CommandId, MachineEnrollment, OrganizationStanding, PersonId};
use fleet_identity_protocol::{
    AgentIsolation, InstallationId, IsolationKind, LaunchContext, MachineEnvironment,
    OrganizationId, SequenceNumber, StartedBy, UnixMillis,
};
use fleet_sync_protocol::ConnectionId;
use message_board::{MachineLabel, ProjectId, ResourceName, SessionRef, WorkspaceId};
use schemars::JsonSchema;
use schemars::{Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadMachinePresence {
    pub installation: InstallationId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachinePresenceAnswer {
    Open {
        #[schemars(schema_with = "connection_id_schema")]
        connection_id: ConnectionId,
        attach_sequence: SequenceNumber,
        open_for_ms: u64,
        silent_for_ms: u64,
    },
    NotConnected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        closed_ago_ms: Option<u64>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WhoIsLoggedIn {
    pub organization_id: OrganizationId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WhoIsLoggedInResult {
    Snapshot {
        as_of: UnixMillis,
        machines: Vec<MachineRow>,
        people: Vec<AdmittedPersonRow>,
    },
    Refused {
        reason: OwnerRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OwnerRefusal {
    NotOwner {},
    NotFound {},
    Conflict {},
    RecordUnwritable {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineRow {
    pub installation: InstallationId,
    pub label: MachineLabel,
    pub enrollment: EnrollmentView,
    pub may_serve: Vec<WorkspaceRef>,
    pub connection: ConnectionView,
    pub sessions: Vec<SessionRow>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EnrollmentView {
    Enrolled {
        approved_by: PersonId,
        approved_at: UnixMillis,
    },
    Revoked {
        approved_by: PersonId,
        approved_at: UnixMillis,
    },
    Expired {
        approved_by: PersonId,
        approved_at: UnixMillis,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConnectionView {
    Connected {
        since: UnixMillis,
        last_heard_at: UnixMillis,
    },
    NotConnected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_seen_at: Option<UnixMillis>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRow {
    pub session: SessionRef,
    pub project: ProjectRef,
    pub workspace: WorkspaceRef,
    pub isolation_kind: IsolationKind,
    pub started_at: UnixMillis,
    pub state: SessionStateView,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SessionStateView {
    Working,
    Idle,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRef {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRef {
    pub project_id: ProjectId,
    pub name: ResourceName,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmittedPersonRow {
    pub person_id: PersonId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "subject",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IdentityLookup {
    Person {
        organization_id: OrganizationId,
        person_id: PersonId,
    },
    Session {
        organization_id: OrganizationId,
        session: SessionRef,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceLookup {
    pub organization_id: OrganizationId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineLookup {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandStatus {
    pub organization_id: OrganizationId,
    pub command_id: CommandId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IdentityLookupResult {
    Person {
        person_id: PersonId,
        standing: OrganizationStanding,
    },
    Session {
        session: Box<SessionRef>,
        machine: InstallationId,
        project: ProjectRef,
        workspace: WorkspaceRef,
        isolation: AgentIsolation,
        machine_environment: MachineEnvironment,
        launch_context: Box<LaunchContext>,
        started_by: Box<StartedBy>,
        started_at: UnixMillis,
    },
    Unknown {},
    Refused {
        reason: OwnerRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceLookupResult {
    Workspace {
        workspace: WorkspaceRef,
        allowed_machines: Vec<AllowedMachineRow>,
    },
    Unknown {},
    Refused {
        reason: OwnerRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowedMachineRow {
    pub installation: InstallationId,
    pub label: MachineLabel,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineLookupResult {
    Machine {
        installation: InstallationId,
        organization_id: OrganizationId,
        label: MachineLabel,
        enrollment: Box<MachineEnrollment>,
        approved_at: UnixMillis,
        allowed_workspaces: Vec<WorkspaceRef>,
        shared_bucket: Vec<WorkspaceRef>,
    },
    Unknown {},
    Refused {
        reason: OwnerRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandStatusResult {
    Pending {},
    Applied {},
    Unknown {},
    Refused { reason: OwnerRefusal },
}

fn connection_id_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$","not":{"const":"00000000-0000-0000-0000-000000000000"}})
}
