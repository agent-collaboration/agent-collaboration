//! Validated registrar facts for the Router's private state; policy has one writer in the registrar.
use collaboration_protocol::{EndpointRef, OperationId, SessionSetupAbandonmentCause};
use fleet_identity_protocol::{
    AgentIsolation, InstallationId, LaunchContext, MachineEnvironment, OrganizationId,
    RegisterFleetSession, StartedBy,
};
use message_board::{ProjectId, RecordId, SessionRef, WorkspaceId};
use serde::{Deserialize, Serialize};

mod binding_address;
pub use binding_address::{
    InvalidSessionBindingAddress, SessionBindingAddress, SessionBindingOrigin,
};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FleetSessionRegistrationId(RecordId);
impl TryFrom<String> for FleetSessionRegistrationId {
    type Error = message_board::IdentityValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.try_into().map(Self)
    }
}
impl FleetSessionRegistrationId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedFleetSession {
    pub endpoint: EndpointRef,
    pub project_id: ProjectId,
    pub checked_workspace: WorkspaceId,
    pub started_by: StartedBy,
    pub isolation: AgentIsolation,
    pub launch_context: LaunchContext,
    pub machine_environment: MachineEnvironment,
}
impl PreparedFleetSession {
    #[must_use]
    pub fn registration(&self, session: SessionRef) -> RegisterFleetSession {
        RegisterFleetSession {
            session,
            project_id: self.project_id.clone(),
            started_by: self.started_by.clone(),
            isolation: self.isolation.clone(),
            launch_context: self.launch_context.clone(),
            machine_environment: self.machine_environment.clone(),
        }
    }
    fn matches_registration(&self, request: &RegisterFleetSession) -> bool {
        request.project_id == self.project_id
            && request.started_by == self.started_by
            && request.isolation == self.isolation
            && request.launch_context == self.launch_context
            && request.machine_environment == self.machine_environment
            && self.matches_session(&request.session)
    }
    fn matches_session(&self, session: &SessionRef) -> bool {
        session.endpoint.service_id.as_str() == String::from(self.endpoint.service_id.clone())
            && session.endpoint.endpoint_id.as_str()
                == String::from(self.endpoint.endpoint_id.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PreparedSessionCreation {
    NotDispatched {},
    Dispatched {},
    CreatedPendingSetup {
        session: SessionRef,
    },
    Created {
        registration: Box<RegisterFleetSession>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AbandonedSessionCreation {
    NotCreated {},
    KnownCreated {
        session: SessionRef,
        cause: SessionSetupAbandonmentCause,
    },
    RegistrationRefused {
        registration: Box<RegisterFleetSession>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisteredFleetSession {
    pub registration: RegisterFleetSession,
    pub organization_id: OrganizationId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetSessionBindingLifecycle {
    Prepared { creation: PreparedSessionCreation },
    Active { confirmed: RegisteredFleetSession },
    Abandoned { creation: AbandonedSessionCreation },
    Retired { confirmed: RegisteredFleetSession },
}
impl FleetSessionBindingLifecycle {
    #[must_use]
    pub fn registration(&self) -> Option<&RegisterFleetSession> {
        match self {
            Self::Prepared {
                creation: PreparedSessionCreation::Created { registration },
            }
            | Self::Abandoned {
                creation: AbandonedSessionCreation::RegistrationRefused { registration },
            } => Some(registration),
            Self::Active { confirmed } | Self::Retired { confirmed } => {
                Some(&confirmed.registration)
            }
            Self::Prepared {
                creation:
                    PreparedSessionCreation::NotDispatched {}
                    | PreparedSessionCreation::Dispatched {}
                    | PreparedSessionCreation::CreatedPendingSetup { .. },
            }
            | Self::Abandoned {
                creation:
                    AbandonedSessionCreation::NotCreated {}
                    | AbandonedSessionCreation::KnownCreated { .. },
            } => None,
        }
    }
    #[must_use]
    pub fn session(&self) -> Option<&SessionRef> {
        match self {
            Self::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session },
            }
            | Self::Abandoned {
                creation: AbandonedSessionCreation::KnownCreated { session, .. },
            } => Some(session),
            _ => self
                .registration()
                .map(|registration| &registration.session),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetSessionPreparation {
    pub registration_id: FleetSessionRegistrationId,
    pub operation_id: OperationId,
    pub installation: InstallationId,
    pub binding_address: SessionBindingAddress,
    pub launch: PreparedFleetSession,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "StoredRegistrationEnvelope",
    into = "StoredRegistrationEnvelope"
)]
pub struct FleetSessionRegistrationRecord {
    preparation: FleetSessionPreparation,
    lifecycle: FleetSessionBindingLifecycle,
}

// The existing lifecycle_json column owns this whole value. This representation
// is private storage, not a Fleet protocol contract or public JSON Schema.
#[derive(Serialize, Deserialize)]
#[serde(tag = "version", deny_unknown_fields)]
enum StoredRegistrationEnvelope {
    #[serde(rename = "1")]
    VersionOne {
        preparation: FleetSessionPreparation,
        lifecycle: FleetSessionBindingLifecycle,
    },
}

impl From<FleetSessionRegistrationRecord> for StoredRegistrationEnvelope {
    fn from(record: FleetSessionRegistrationRecord) -> Self {
        Self::VersionOne {
            preparation: record.preparation,
            lifecycle: record.lifecycle,
        }
    }
}

impl TryFrom<StoredRegistrationEnvelope> for FleetSessionRegistrationRecord {
    type Error = InvalidFleetSessionRegistration;
    fn try_from(envelope: StoredRegistrationEnvelope) -> Result<Self, Self::Error> {
        match envelope {
            StoredRegistrationEnvelope::VersionOne {
                preparation,
                lifecycle,
            } => Self::new(preparation, lifecycle),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid Fleet session registration: {field}")]
pub struct InvalidFleetSessionRegistration {
    pub field: &'static str,
}

impl FleetSessionRegistrationRecord {
    pub fn new(
        preparation: FleetSessionPreparation,
        lifecycle: FleetSessionBindingLifecycle,
    ) -> Result<Self, InvalidFleetSessionRegistration> {
        if String::from(preparation.launch.endpoint.service_id.clone())
            != preparation.installation.as_str()
        {
            return Err(InvalidFleetSessionRegistration {
                field: "installation",
            });
        }
        if let Some(registration) = lifecycle.registration()
            && !preparation.launch.matches_registration(registration)
        {
            return Err(InvalidFleetSessionRegistration {
                field: "registration",
            });
        }
        if let Some(session) = lifecycle.session()
            && !preparation.launch.matches_session(session)
        {
            return Err(InvalidFleetSessionRegistration { field: "session" });
        }
        if let FleetSessionBindingLifecycle::Active { confirmed }
        | FleetSessionBindingLifecycle::Retired { confirmed } = &lifecycle
            && confirmed.workspace_id != preparation.launch.checked_workspace
        {
            return Err(InvalidFleetSessionRegistration { field: "workspace" });
        }
        Ok(Self {
            preparation,
            lifecycle,
        })
    }
    #[must_use]
    pub fn preparation(&self) -> &FleetSessionPreparation {
        &self.preparation
    }
    #[must_use]
    pub fn lifecycle(&self) -> &FleetSessionBindingLifecycle {
        &self.lifecycle
    }
    pub fn with_lifecycle(
        &self,
        lifecycle: FleetSessionBindingLifecycle,
    ) -> Result<Self, InvalidFleetSessionRegistration> {
        Self::new(self.preparation.clone(), lifecycle)
    }
}

#[cfg(test)]
#[path = "fleet_session_registration_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "stored_registration_envelope_tests.rs"]
mod storage_tests;
