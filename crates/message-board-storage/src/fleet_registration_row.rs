//! Typed columns decode through the same registrar constructors as incoming values.
use crate::fleet_session_registration::{
    AbandonedSessionCreation, FleetSessionBindingLifecycle, FleetSessionPreparation,
    FleetSessionRegistrationRecord, PreparedFleetSession, PreparedSessionCreation,
    RegisteredFleetSession,
};
use crate::session_registration_repository::FleetSessionStorageFailure;
use collaboration_protocol::{EndpointRef, ProviderSettingName, SessionSetupAbandonmentCause};
use fleet_identity_protocol::{
    AgentIsolation, IsolationKind, LaunchContext, MachineEnvironment, StartedBy,
};
use message_board::{SessionEndpointRef, SessionRef};

#[derive(Eq, PartialEq)]
pub(super) struct StoredRegistrationRow {
    pub(super) registration_id: String,
    pub(super) operation_id: String,
    pub(super) installation_id: String,
    pub(super) binding_address: String,
    pub(super) endpoint_service_id: String,
    pub(super) endpoint_id: String,
    pub(super) project_id: String,
    pub(super) checked_workspace_id: String,
    pub(super) started_by_kind: String,
    pub(super) started_by_service_id: Option<String>,
    pub(super) started_by_endpoint_id: Option<String>,
    pub(super) started_by_session_id: Option<String>,
    pub(super) remote_start_id: Option<String>,
    pub(super) isolation_kind: String,
    pub(super) isolation_instance_id: String,
    pub(super) working_directory: String,
    pub(super) repository: Option<String>,
    pub(super) commit_id: Option<String>,
    pub(super) model: Option<String>,
    pub(super) machine_environment: String,
    pub(super) lifecycle_state: String,
    pub(super) creation_state: Option<String>,
    pub(super) session_service_id: Option<String>,
    pub(super) session_endpoint_id: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) organization_id: Option<String>,
    pub(super) abandonment_cause: Option<String>,
    pub(super) refused_setting: Option<String>,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
}
fn invalid() -> FleetSessionStorageFailure {
    FleetSessionStorageFailure::InvalidRecord
}
fn required(value: Option<&String>) -> Result<&str, FleetSessionStorageFailure> {
    value.map(String::as_str).ok_or_else(invalid)
}
fn session(
    service: Option<&String>,
    endpoint: Option<&String>,
    id: Option<&String>,
) -> Result<SessionRef, FleetSessionStorageFailure> {
    Ok(SessionRef {
        endpoint: SessionEndpointRef {
            service_id: required(service)?
                .to_owned()
                .try_into()
                .map_err(|_| invalid())?,
            endpoint_id: required(endpoint)?
                .to_owned()
                .try_into()
                .map_err(|_| invalid())?,
        },
        session_id: required(id)?.to_owned().try_into().map_err(|_| invalid())?,
    })
}
impl StoredRegistrationRow {
    pub(super) fn encode(
        record: &FleetSessionRegistrationRecord,
        created_at: i64,
        updated_at: i64,
    ) -> Result<Self, FleetSessionStorageFailure> {
        if created_at < 0 || updated_at < created_at {
            return Err(invalid());
        }
        let record = FleetSessionRegistrationRecord::new(
            record.preparation().clone(),
            record.lifecycle().clone(),
        )
        .map_err(|_| invalid())?;
        let preparation = record.preparation();
        let launch = &preparation.launch;
        let (started_by_kind, started_session, remote_start_id) = match &launch.started_by {
            StartedBy::RouterOperator {} => ("routerOperator", None, None),
            StartedBy::Session { session } => ("session", Some(session), None),
            StartedBy::RemoteSession {
                session,
                request_id,
            } => (
                "remoteSession",
                Some(session),
                Some(request_id.as_str().to_owned()),
            ),
        };
        let (lifecycle_state, creation_state, organization_id, cause) = match record.lifecycle() {
            FleetSessionBindingLifecycle::Prepared { creation } => (
                "prepared",
                Some(match creation {
                    PreparedSessionCreation::NotDispatched {} => "notDispatched",
                    PreparedSessionCreation::Dispatched {} => "dispatched",
                    PreparedSessionCreation::CreatedPendingSetup { .. } => "createdPendingSetup",
                    PreparedSessionCreation::Created { .. } => "created",
                }),
                None,
                None,
            ),
            FleetSessionBindingLifecycle::Active { confirmed } => (
                "active",
                None,
                Some(confirmed.organization_id.as_str().to_owned()),
                None,
            ),
            FleetSessionBindingLifecycle::Retired { confirmed } => (
                "retired",
                None,
                Some(confirmed.organization_id.as_str().to_owned()),
                None,
            ),
            FleetSessionBindingLifecycle::Abandoned { creation } => (
                "abandoned",
                Some(match creation {
                    AbandonedSessionCreation::NotCreated {} => "notCreated",
                    AbandonedSessionCreation::KnownCreated { .. } => "knownCreated",
                    AbandonedSessionCreation::RegistrationRefused { .. } => "registrationRefused",
                }),
                None,
                match creation {
                    AbandonedSessionCreation::KnownCreated { cause, .. } => Some(cause),
                    _ => None,
                },
            ),
        };
        let (abandonment_cause, refused_setting) = match cause {
            None => (None, None),
            Some(SessionSetupAbandonmentCause::ModelMismatch {}) => (Some("modelMismatch"), None),
            Some(SessionSetupAbandonmentCause::EffectiveWorkingDirectoryMismatch {}) => {
                (Some("workingDirectoryMismatch"), None)
            }
            Some(SessionSetupAbandonmentCause::WorkspaceRootsMismatch {}) => {
                (Some("workspaceRootsMismatch"), None)
            }
            Some(SessionSetupAbandonmentCause::SetupInterrupted {}) => {
                (Some("setupInterrupted"), None)
            }
            Some(SessionSetupAbandonmentCause::InitialSettingRefused { setting }) => (
                Some("initialSettingRefused"),
                Some(match setting {
                    ProviderSettingName::Mode => "mode",
                    ProviderSettingName::Model => "model",
                    ProviderSettingName::Effort => "effort",
                }),
            ),
        };
        let actual_session = record.lifecycle().session();
        Ok(Self {
            registration_id: preparation.registration_id.as_str().to_owned(),
            operation_id: preparation.operation_id.as_str().to_owned(),
            installation_id: preparation.installation.as_str().to_owned(),
            binding_address: preparation.binding_address.expose_address().to_owned(),
            endpoint_service_id: String::from(launch.endpoint.service_id.clone()),
            endpoint_id: String::from(launch.endpoint.endpoint_id.clone()),
            project_id: launch.project_id.as_str().to_owned(),
            checked_workspace_id: launch.checked_workspace.as_str().to_owned(),
            started_by_kind: started_by_kind.to_owned(),
            started_by_service_id: started_session
                .map(|session| session.endpoint.service_id.as_str().to_owned()),
            started_by_endpoint_id: started_session
                .map(|session| session.endpoint.endpoint_id.as_str().to_owned()),
            started_by_session_id: started_session
                .map(|session| session.session_id.as_str().to_owned()),
            remote_start_id,
            isolation_kind: match launch.isolation.kind {
                IsolationKind::Host => "host",
                IsolationKind::ProcessSandbox => "processSandbox",
                IsolationKind::Container => "container",
                IsolationKind::Vm => "vm",
            }
            .to_owned(),
            isolation_instance_id: launch.isolation.instance_id.as_str().to_owned(),
            working_directory: launch.launch_context.working_directory.as_str().to_owned(),
            repository: launch
                .launch_context
                .repository
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            commit_id: launch
                .launch_context
                .commit
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            model: launch
                .launch_context
                .model
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            machine_environment: match launch.machine_environment {
                MachineEnvironment::Physical => "physical",
                MachineEnvironment::CloudVm => "cloudVm",
            }
            .to_owned(),
            lifecycle_state: lifecycle_state.to_owned(),
            creation_state: creation_state.map(str::to_owned),
            session_service_id: actual_session
                .map(|session| session.endpoint.service_id.as_str().to_owned()),
            session_endpoint_id: actual_session
                .map(|session| session.endpoint.endpoint_id.as_str().to_owned()),
            session_id: actual_session.map(|session| session.session_id.as_str().to_owned()),
            organization_id,
            abandonment_cause: abandonment_cause.map(str::to_owned),
            refused_setting: refused_setting.map(str::to_owned),
            created_at,
            updated_at,
        })
    }
    pub(super) fn decode(
        self,
    ) -> Result<FleetSessionRegistrationRecord, FleetSessionStorageFailure> {
        let started_by = match self.started_by_kind.as_str() {
            "routerOperator" => StartedBy::RouterOperator {},
            "session" => StartedBy::Session {
                session: session(
                    self.started_by_service_id.as_ref(),
                    self.started_by_endpoint_id.as_ref(),
                    self.started_by_session_id.as_ref(),
                )?,
            },
            "remoteSession" => StartedBy::RemoteSession {
                session: session(
                    self.started_by_service_id.as_ref(),
                    self.started_by_endpoint_id.as_ref(),
                    self.started_by_session_id.as_ref(),
                )?,
                request_id: required(self.remote_start_id.as_ref())?
                    .to_owned()
                    .try_into()
                    .map_err(|_| invalid())?,
            },
            _ => return Err(invalid()),
        };
        let launch = PreparedFleetSession {
            endpoint: EndpointRef {
                service_id: self
                    .endpoint_service_id
                    .clone()
                    .try_into()
                    .map_err(|_| invalid())?,
                endpoint_id: self.endpoint_id.clone().try_into().map_err(|_| invalid())?,
            },
            project_id: self.project_id.clone().try_into().map_err(|_| invalid())?,
            checked_workspace: self
                .checked_workspace_id
                .clone()
                .try_into()
                .map_err(|_| invalid())?,
            started_by,
            isolation: AgentIsolation {
                kind: match self.isolation_kind.as_str() {
                    "host" => IsolationKind::Host,
                    "processSandbox" => IsolationKind::ProcessSandbox,
                    "container" => IsolationKind::Container,
                    "vm" => IsolationKind::Vm,
                    _ => return Err(invalid()),
                },
                instance_id: self
                    .isolation_instance_id
                    .clone()
                    .try_into()
                    .map_err(|_| invalid())?,
            },
            launch_context: LaunchContext {
                working_directory: self
                    .working_directory
                    .clone()
                    .try_into()
                    .map_err(|_| invalid())?,
                repository: self
                    .repository
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| invalid())?,
                commit: self
                    .commit_id
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| invalid())?,
                model: self
                    .model
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| invalid())?,
            },
            machine_environment: match self.machine_environment.as_str() {
                "physical" => MachineEnvironment::Physical,
                "cloudVm" => MachineEnvironment::CloudVm,
                _ => return Err(invalid()),
            },
        };
        let actual = || {
            session(
                self.session_service_id.as_ref(),
                self.session_endpoint_id.as_ref(),
                self.session_id.as_ref(),
            )
        };
        let cause = || match self.abandonment_cause.as_deref() {
            Some("modelMismatch") => Ok(SessionSetupAbandonmentCause::ModelMismatch {}),
            Some("workingDirectoryMismatch") => {
                Ok(SessionSetupAbandonmentCause::EffectiveWorkingDirectoryMismatch {})
            }
            Some("workspaceRootsMismatch") => {
                Ok(SessionSetupAbandonmentCause::WorkspaceRootsMismatch {})
            }
            Some("setupInterrupted") => Ok(SessionSetupAbandonmentCause::SetupInterrupted {}),
            Some("initialSettingRefused") => {
                Ok(SessionSetupAbandonmentCause::InitialSettingRefused {
                    setting: match self.refused_setting.as_deref() {
                        Some("mode") => ProviderSettingName::Mode,
                        Some("model") => ProviderSettingName::Model,
                        Some("effort") => ProviderSettingName::Effort,
                        _ => return Err(invalid()),
                    },
                })
            }
            _ => Err(invalid()),
        };
        let lifecycle = match (
            self.lifecycle_state.as_str(),
            self.creation_state.as_deref(),
        ) {
            ("prepared", Some("notDispatched")) => FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {},
            },
            ("prepared", Some("dispatched")) => FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Dispatched {},
            },
            ("prepared", Some("createdPendingSetup")) => FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session: actual()? },
            },
            ("prepared", Some("created")) => FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Created {
                    registration: Box::new(launch.registration(actual()?)),
                },
            },
            ("active", None) | ("retired", None) => {
                let confirmed = RegisteredFleetSession {
                    registration: launch.registration(actual()?),
                    organization_id: required(self.organization_id.as_ref())?
                        .to_owned()
                        .try_into()
                        .map_err(|_| invalid())?,
                    workspace_id: launch.checked_workspace.clone(),
                };
                if self.lifecycle_state == "active" {
                    FleetSessionBindingLifecycle::Active { confirmed }
                } else {
                    FleetSessionBindingLifecycle::Retired { confirmed }
                }
            }
            ("abandoned", Some("notCreated")) => FleetSessionBindingLifecycle::Abandoned {
                creation: AbandonedSessionCreation::NotCreated {},
            },
            ("abandoned", Some("knownCreated")) => FleetSessionBindingLifecycle::Abandoned {
                creation: AbandonedSessionCreation::KnownCreated {
                    session: actual()?,
                    cause: cause()?,
                },
            },
            ("abandoned", Some("registrationRefused")) => FleetSessionBindingLifecycle::Abandoned {
                creation: AbandonedSessionCreation::RegistrationRefused {
                    registration: Box::new(launch.registration(actual()?)),
                },
            },
            _ => return Err(invalid()),
        };
        let preparation = FleetSessionPreparation {
            registration_id: self
                .registration_id
                .clone()
                .try_into()
                .map_err(|_| invalid())?,
            operation_id: self
                .operation_id
                .clone()
                .try_into()
                .map_err(|_| invalid())?,
            installation: self
                .installation_id
                .clone()
                .try_into()
                .map_err(|_| invalid())?,
            binding_address: self
                .binding_address
                .clone()
                .try_into()
                .map_err(|_| invalid())?,
            launch,
        };
        let record =
            FleetSessionRegistrationRecord::new(preparation, lifecycle).map_err(|_| invalid())?;
        // Reject unused payloads and partial triples as well as malformed individual fields.
        if Self::encode(&record, self.created_at, self.updated_at)? != self {
            return Err(invalid());
        }
        Ok(record)
    }
}

#[cfg(test)]
#[path = "fleet_registration_row_test.rs"]
mod tests;
