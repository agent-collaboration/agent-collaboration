//! Validated registrar facts for the Router's private state; policy has one writer in the registrar.
use collaboration_protocol::{EndpointRef, OperationId};
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
            && request.session.endpoint.service_id.as_str()
                == String::from(self.endpoint.service_id.clone())
            && request.session.endpoint.endpoint_id.as_str()
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
                    PreparedSessionCreation::NotDispatched {} | PreparedSessionCreation::Dispatched {},
            }
            | Self::Abandoned {
                creation: AbandonedSessionCreation::NotCreated {},
            } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetSessionPreparation {
    pub registration_id: FleetSessionRegistrationId,
    pub operation_id: OperationId,
    pub installation: InstallationId,
    pub binding_address: SessionBindingAddress,
    pub launch: PreparedFleetSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetSessionRegistrationRecord {
    preparation: FleetSessionPreparation,
    lifecycle: FleetSessionBindingLifecycle,
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
