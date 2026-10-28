//! Authority belongs to a project or workstream, independently of conversation seats.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    HumanId, Identity, MachineLabel, ProjectId, ResourceName, RoleAssignmentId, ServiceId,
    SessionRef, WorkstreamId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorOrigin {
    pub installation: ServiceId,
    pub machine: MachineLabel,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RecordedActor {
    Human {
        human_id: HumanId,
    },
    Session {
        session: SessionRef,
    },
    Router {
        component: RouterComponent,
        installation: ServiceId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(from = "ActorKindWire", into = "ActorKindWire")]
pub enum ActorKind {
    Declared(Identity),
    Router { component: RouterComponent },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ActorKindWire {
    Declared { identity: Identity },
    Router { component: RouterComponent },
}

impl From<ActorKindWire> for ActorKind {
    fn from(actor: ActorKindWire) -> Self {
        match actor {
            ActorKindWire::Declared { identity } => Self::Declared(identity),
            ActorKindWire::Router { component } => Self::Router { component },
        }
    }
}
impl From<ActorKind> for ActorKindWire {
    fn from(actor: ActorKind) -> Self {
        match actor {
            ActorKind::Declared(identity) => Self::Declared { identity },
            ActorKind::Router { component } => Self::Router { component },
        }
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum RouterComponent {
    SubscriptionDelivery,
    ArchiveObservation,
}

impl RecordedActor {
    #[must_use]
    pub fn declared(identity: Identity) -> Self {
        match identity {
            Identity::Human { human_id } => Self::Human { human_id },
            Identity::Session { session } => Self::Session { session },
        }
    }

    #[must_use]
    pub fn declared_identity(&self) -> Option<Identity> {
        match self {
            Self::Human { human_id } => Some(Identity::Human {
                human_id: human_id.clone(),
            }),
            Self::Session { session, .. } => Some(Identity::Session {
                session: session.clone(),
            }),
            Self::Router { .. } => None,
        }
    }

    #[must_use]
    pub fn kind(&self) -> ActorKind {
        match self {
            Self::Human { human_id } => ActorKind::Declared(Identity::Human {
                human_id: human_id.clone(),
            }),
            Self::Session { session, .. } => ActorKind::Declared(Identity::Session {
                session: session.clone(),
            }),
            Self::Router { component, .. } => ActorKind::Router {
                component: *component,
            },
        }
    }

    #[must_use]
    pub fn installation(&self) -> Option<&ServiceId> {
        match self {
            Self::Human { .. } => None,
            Self::Session { session } => Some(&session.endpoint.service_id),
            Self::Router { installation, .. } => Some(installation),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AuthorityRole {
    Lead,
    Advisor,
    Implementer,
    Verifier,
    Reviewer,
    Operator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RoleCardinality {
    One,
    Many,
}

impl AuthorityRole {
    #[must_use]
    pub const fn cardinality(self) -> RoleCardinality {
        match self {
            Self::Lead | Self::Implementer | Self::Verifier => RoleCardinality::One,
            Self::Advisor | Self::Reviewer | Self::Operator => RoleCardinality::Many,
        }
    }
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Advisor => "advisor",
            Self::Implementer => "implementer",
            Self::Verifier => "verifier",
            Self::Reviewer => "reviewer",
            Self::Operator => "operator",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "role",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RoleRequirement {
    Holds(AuthorityRole),
    LeadOrWorkstreamImplementer,
    AnyProjectRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "subject",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RoleSubject {
    Project(ProjectId),
    Workstream(WorkstreamId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "subject",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Assignment {
    Lead(ProjectId),
    Advisor(ProjectId),
    Implementer(WorkstreamId),
    Verifier(WorkstreamId),
    Reviewer(WorkstreamId),
    Operator(WorkstreamId),
}

impl Assignment {
    pub fn new(
        subject: RoleSubject,
        role: AuthorityRole,
    ) -> Result<Self, RoleAssignmentValidationError> {
        Ok(match (subject, role) {
            (RoleSubject::Project(project), AuthorityRole::Lead) => Assignment::Lead(project),
            (RoleSubject::Project(project), AuthorityRole::Advisor) => Assignment::Advisor(project),
            (RoleSubject::Workstream(workstream), AuthorityRole::Reviewer) => {
                Assignment::Reviewer(workstream)
            }
            (RoleSubject::Workstream(workstream), AuthorityRole::Operator) => {
                Assignment::Operator(workstream)
            }
            (RoleSubject::Workstream(workstream), AuthorityRole::Implementer) => {
                Assignment::Implementer(workstream)
            }
            (RoleSubject::Workstream(workstream), AuthorityRole::Verifier) => {
                Assignment::Verifier(workstream)
            }
            (subject, role) => {
                return Err(RoleAssignmentValidationError::InvalidSubject { role, subject });
            }
        })
    }

    #[must_use]
    pub fn subject(&self) -> RoleSubject {
        match self {
            Self::Lead(project) | Self::Advisor(project) => RoleSubject::Project(project.clone()),
            Self::Implementer(workstream)
            | Self::Verifier(workstream)
            | Self::Reviewer(workstream)
            | Self::Operator(workstream) => RoleSubject::Workstream(workstream.clone()),
        }
    }
    #[must_use]
    pub const fn role(&self) -> AuthorityRole {
        match self {
            Self::Lead(_) => AuthorityRole::Lead,
            Self::Advisor(_) => AuthorityRole::Advisor,
            Self::Implementer(_) => AuthorityRole::Implementer,
            Self::Verifier(_) => AuthorityRole::Verifier,
            Self::Reviewer(_) => AuthorityRole::Reviewer,
            Self::Operator(_) => AuthorityRole::Operator,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RoleResolution {
    Holders {
        actors: std::collections::BTreeSet<RecordedActor>,
        revision: crate::AssignmentRevision,
    },
    NoHolder {
        revision: crate::AssignmentRevision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "recipient",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum NotifyTarget {
    Actor(Identity),
    Role(Assignment),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RoleEnd {
    Released {},
    Replaced {},
    StoodDown { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RoleAssignmentValidationError {
    #[error("role {role:?} cannot be assigned to {subject:?}")]
    InvalidSubject {
        role: AuthorityRole,
        subject: RoleSubject,
    },
    #[error("a Router recording actor cannot hold authority")]
    RouterHolder,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleAssignmentProps {
    pub envelope: crate::RecordEnvelope,
    pub assignment_id: RoleAssignmentId,
    pub subject: RoleSubject,
    pub role: AuthorityRole,
    pub holder: RecordedActor,
    pub granted_by: RecordedActor,
    pub model: Option<ResourceName>,
    pub lineage: Option<ResourceName>,
    pub ended: Option<RoleEnd>,
    pub owner_grant: Option<crate::OwnerGrantId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RoleAssignmentProps", into = "RoleAssignmentProps")]
pub struct RoleAssignment {
    pub envelope: crate::RecordEnvelope,
    pub assignment_id: RoleAssignmentId,
    assignment: Assignment,
    pub holder: RecordedActor,
    pub granted_by: RecordedActor,
    pub model: Option<ResourceName>,
    pub lineage: Option<ResourceName>,
    pub ended: Option<RoleEnd>,
    pub owner_grant: Option<crate::OwnerGrantId>,
}

impl RoleAssignment {
    pub fn new(props: RoleAssignmentProps) -> Result<Self, RoleAssignmentValidationError> {
        if props.holder.declared_identity().is_none() {
            return Err(RoleAssignmentValidationError::RouterHolder);
        }
        let assignment = Assignment::new(props.subject, props.role)?;
        Ok(Self {
            envelope: props.envelope,
            assignment_id: props.assignment_id,
            assignment,
            holder: props.holder,
            granted_by: props.granted_by,
            model: props.model,
            lineage: props.lineage,
            ended: props.ended,
            owner_grant: props.owner_grant,
        })
    }
    #[must_use]
    pub fn assignment(&self) -> Assignment {
        self.assignment.clone()
    }
    #[must_use]
    pub fn subject(&self) -> RoleSubject {
        match &self.assignment {
            Assignment::Lead(project) | Assignment::Advisor(project) => {
                RoleSubject::Project(project.clone())
            }
            Assignment::Implementer(workstream)
            | Assignment::Verifier(workstream)
            | Assignment::Reviewer(workstream)
            | Assignment::Operator(workstream) => RoleSubject::Workstream(workstream.clone()),
        }
    }
    #[must_use]
    pub fn role(&self) -> AuthorityRole {
        match self.assignment {
            Assignment::Lead(_) => AuthorityRole::Lead,
            Assignment::Advisor(_) => AuthorityRole::Advisor,
            Assignment::Reviewer(_) => AuthorityRole::Reviewer,
            Assignment::Operator(_) => AuthorityRole::Operator,
            Assignment::Implementer(_) => AuthorityRole::Implementer,
            Assignment::Verifier(_) => AuthorityRole::Verifier,
        }
    }
}
impl TryFrom<RoleAssignmentProps> for RoleAssignment {
    type Error = RoleAssignmentValidationError;
    fn try_from(props: RoleAssignmentProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<RoleAssignment> for RoleAssignmentProps {
    fn from(record: RoleAssignment) -> Self {
        Self {
            subject: record.subject(),
            role: record.role(),
            envelope: record.envelope,
            assignment_id: record.assignment_id,
            holder: record.holder,
            granted_by: record.granted_by,
            model: record.model,
            lineage: record.lineage,
            ended: record.ended,
            owner_grant: record.owner_grant,
        }
    }
}

impl Ord for RecordedActor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.kind(), self.installation()).cmp(&(other.kind(), other.installation()))
    }
}
impl PartialOrd for RecordedActor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
