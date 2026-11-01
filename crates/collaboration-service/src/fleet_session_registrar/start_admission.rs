//! Start admission reads live authority before minting a prepared binding or creating anything.
use std::sync::Arc;

use codex_router_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, FleetSessionPreparation, FleetSessionRegistrationRecord,
    PreparedFleetSession, PreparedSessionCreation, SessionBindingOrigin,
};
use collaboration_protocol::{EndpointRef, OperationId};
use fleet_identity_protocol::{
    AgentIsolation, CheckSessionStart, InstallationId, LaunchContext, MachineEnvironment,
    SessionStartCheck, SessionStartRefusal, StartedBy,
};
use message_board::ProjectId;
use runtime_primitives::IdSource;

use super::{
    FleetSessionConnection, FleetSessionConnectionFailure, FleetSessionRegistrationRepository,
    FleetSessionStorageFailure, RegistrationInsert,
};

/// F11 supplies this policy projection; F17 removes the AccessEnrollment branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRegistrationMode {
    AccessEnrollment,
    MachineTokens,
}
pub trait SessionRegistrationModeSource: Send + Sync {
    fn mode(&self) -> SessionRegistrationMode;
}
pub trait SessionBindingEntropy: Send + Sync {
    fn new_capability(&self) -> Result<[u8; 16], SessionBindingEntropyUnavailable>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session binding entropy unavailable")]
pub struct SessionBindingEntropyUnavailable;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetSessionLaunch {
    pub operation_id: OperationId,
    pub endpoint: EndpointRef,
    pub project_id: Option<ProjectId>,
    pub started_by: StartedBy,
    pub isolation: AgentIsolation,
    pub launch_context: LaunchContext,
    pub machine_environment: MachineEnvironment,
}

pub struct FleetSessionRegistrarProps {
    pub installation: InstallationId,
    pub origin: SessionBindingOrigin,
    pub repository: Arc<dyn FleetSessionRegistrationRepository>,
    pub connection: Arc<dyn FleetSessionConnection>,
    pub mode: Arc<dyn SessionRegistrationModeSource>,
    pub ids: Arc<dyn IdSource + Send + Sync>,
    pub entropy: Arc<dyn SessionBindingEntropy>,
}
pub struct FleetSessionRegistrar {
    pub(super) installation: InstallationId,
    pub(super) origin: SessionBindingOrigin,
    pub(super) repository: Arc<dyn FleetSessionRegistrationRepository>,
    pub(super) connection: Arc<dyn FleetSessionConnection>,
    mode: Arc<dyn SessionRegistrationModeSource>,
    ids: Arc<dyn IdSource + Send + Sync>,
    entropy: Arc<dyn SessionBindingEntropy>,
}

#[derive(Debug)]
pub enum PrepareSessionStart {
    AccessEnrollment,
    Prepared(Box<FleetSessionRegistrationRecord>),
    Existing(Box<FleetSessionRegistrationRecord>),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RegistrarFailure {
    #[error("a session start must name its project")]
    ProjectRequired,
    #[error("session start was refused by the organization authority")]
    StartRefused(SessionStartRefusal),
    #[error("session registration belongs to another installation")]
    ForeignInstallation,
    #[error("the configured session binding origin changed")]
    OriginChanged,
    #[error("session create operation has different immutable input")]
    OperationConflict,
    #[error("session registration is unavailable")]
    Unavailable,
    #[error("session registration is not prepared for this transition")]
    InvalidTransition,
    #[error("session registration publication changed concurrently")]
    TransitionConflict,
    #[error("session binding is inactive or unknown")]
    InactiveBinding,
    #[error("the declared sender differs from the bound session")]
    SenderMismatch,
    #[error(transparent)]
    Connection(#[from] FleetSessionConnectionFailure),
    #[error(transparent)]
    Storage(#[from] FleetSessionStorageFailure),
}

impl FleetSessionRegistrar {
    #[must_use]
    pub fn new(props: FleetSessionRegistrarProps) -> Self {
        Self {
            installation: props.installation,
            origin: props.origin,
            repository: props.repository,
            connection: props.connection,
            mode: props.mode,
            ids: props.ids,
            entropy: props.entropy,
        }
    }

    pub async fn prepare(
        &self,
        intent: &FleetSessionLaunch,
    ) -> Result<PrepareSessionStart, RegistrarFailure> {
        if self.mode.mode() == SessionRegistrationMode::AccessEnrollment {
            return Ok(PrepareSessionStart::AccessEnrollment);
        }
        let project_id = intent
            .project_id
            .clone()
            .ok_or(RegistrarFailure::ProjectRequired)?;
        if String::from(intent.endpoint.service_id.clone()) != self.installation.as_str() {
            return Err(RegistrarFailure::ForeignInstallation);
        }
        if let Some(record) = self.repository.operation(&intent.operation_id).await? {
            self.require_record_context(&record)?;
            if !matches_intent(&record, intent) {
                return Err(RegistrarFailure::OperationConflict);
            }
            return Ok(PrepareSessionStart::Existing(Box::new(record)));
        }
        let workspace = match self
            .connection
            .check_start(&CheckSessionStart {
                project_id: project_id.clone(),
                started_by: intent.started_by.clone(),
            })
            .await?
        {
            SessionStartCheck::Allowed { workspace_id } => workspace_id,
            SessionStartCheck::Refused { reason } => {
                return Err(RegistrarFailure::StartRefused(reason));
            }
        };
        let capability = self
            .entropy
            .new_capability()
            .map_err(|_| RegistrarFailure::Unavailable)?;
        let address = format!(
            "{}/mcp/session/{}",
            self.origin.as_str(),
            capability_text(capability)
        )
        .try_into()
        .map_err(|_| RegistrarFailure::Unavailable)?;
        let record = FleetSessionRegistrationRecord::new(
            FleetSessionPreparation {
                registration_id: self
                    .ids
                    .new_id()
                    .to_string()
                    .try_into()
                    .map_err(|_| RegistrarFailure::Unavailable)?,
                operation_id: intent.operation_id.clone(),
                installation: self.installation.clone(),
                binding_address: address,
                launch: PreparedFleetSession {
                    endpoint: intent.endpoint.clone(),
                    project_id,
                    checked_workspace: workspace,
                    started_by: intent.started_by.clone(),
                    isolation: intent.isolation.clone(),
                    launch_context: intent.launch_context.clone(),
                    machine_environment: intent.machine_environment.clone(),
                },
            },
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {},
            },
        )
        .map_err(|_| RegistrarFailure::Unavailable)?;
        match self.repository.insert(&record).await? {
            RegistrationInsert::Inserted => Ok(PrepareSessionStart::Prepared(Box::new(record))),
            RegistrationInsert::Existing(existing) => {
                self.require_record_context(&existing)?;
                if !matches_intent(&existing, intent) {
                    return Err(RegistrarFailure::OperationConflict);
                }
                Ok(PrepareSessionStart::Existing(existing))
            }
        }
    }

    pub(super) fn require_record_context(
        &self,
        record: &FleetSessionRegistrationRecord,
    ) -> Result<(), RegistrarFailure> {
        if record.preparation().installation != self.installation {
            return Err(RegistrarFailure::ForeignInstallation);
        }
        if record
            .preparation()
            .binding_address
            .origin()
            .map_err(|_| RegistrarFailure::Unavailable)?
            != self.origin
        {
            return Err(RegistrarFailure::OriginChanged);
        }
        Ok(())
    }
}

fn matches_intent(record: &FleetSessionRegistrationRecord, intent: &FleetSessionLaunch) -> bool {
    let launch = &record.preparation().launch;
    record.preparation().operation_id == intent.operation_id
        && launch.endpoint == intent.endpoint
        && Some(&launch.project_id) == intent.project_id.as_ref()
        && launch.started_by == intent.started_by
        && launch.isolation == intent.isolation
        && launch.launch_context == intent.launch_context
        && launch.machine_environment == intent.machine_environment
}
fn capability_text(bytes: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(32);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 15)]));
    }
    text
}
