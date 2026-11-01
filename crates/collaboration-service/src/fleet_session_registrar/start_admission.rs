//! Start admission reads live authority before minting a prepared binding or creating anything.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use codex_router_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, FleetSessionPreparation, FleetSessionRegistrationRecord,
    PreparedFleetSession, PreparedSessionCreation, SessionBindingOrigin,
};
use collaboration_contracts::{HarnessLaunchIdentity, HarnessSessionLaunch};
use collaboration_protocol::OperationId;
use fleet_identity_protocol::{
    CheckSessionStart, InstallationId, SessionStartCheck, SessionStartRefusal,
};
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionBindingListener {
    Stable(SessionBindingOrigin),
    Ephemeral,
}
pub trait SessionBindingEntropy: Send + Sync {
    fn new_capability(&self) -> Result<[u8; 16], SessionBindingEntropyUnavailable>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session binding entropy unavailable")]
pub struct SessionBindingEntropyUnavailable;

pub struct FleetSessionRegistrarProps {
    pub installation: InstallationId,
    pub listener: SessionBindingListener,
    pub repository: Arc<dyn FleetSessionRegistrationRepository>,
    pub connection: Arc<dyn FleetSessionConnection>,
    pub mode: Arc<dyn SessionRegistrationModeSource>,
    pub ids: Arc<dyn IdSource + Send + Sync>,
    pub entropy: Arc<dyn SessionBindingEntropy>,
}
pub struct FleetSessionRegistrar {
    pub(super) installation: InstallationId,
    listener: SessionBindingListener,
    pub(super) repository: Arc<dyn FleetSessionRegistrationRepository>,
    pub(super) connection: Arc<dyn FleetSessionConnection>,
    mode: Arc<dyn SessionRegistrationModeSource>,
    ids: Arc<dyn IdSource + Send + Sync>,
    entropy: Arc<dyn SessionBindingEntropy>,
    pub(super) prepared_identities: Mutex<BTreeMap<OperationId, HarnessLaunchIdentity>>,
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
    #[error("session registration requires a fixed MCP listener origin")]
    EphemeralListener,
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
            listener: props.listener,
            repository: props.repository,
            connection: props.connection,
            mode: props.mode,
            ids: props.ids,
            entropy: props.entropy,
            prepared_identities: Mutex::new(BTreeMap::new()),
        }
    }

    pub async fn prepare(
        &self,
        intent: &HarnessSessionLaunch,
        identity: &HarnessLaunchIdentity,
    ) -> Result<PrepareSessionStart, RegistrarFailure> {
        if self.mode.mode() == SessionRegistrationMode::AccessEnrollment {
            return Ok(PrepareSessionStart::AccessEnrollment);
        }
        let project_id = intent
            .project_id
            .clone()
            .ok_or(RegistrarFailure::ProjectRequired)?;
        let origin = self.stable_origin()?;
        if String::from(intent.endpoint.service_id.clone()) != self.installation.as_str() {
            return Err(RegistrarFailure::ForeignInstallation);
        }
        if let Some(record) = self.repository.operation(&intent.operation_id).await? {
            self.require_record_context(&record)?;
            self.require_prepared_identity(&record, identity)?;
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
            origin.as_str(),
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
            RegistrationInsert::Inserted => {
                self.prepared_identities
                    .lock()
                    .map_err(|_| RegistrarFailure::Unavailable)?
                    .insert(intent.operation_id.clone(), identity.clone());
                Ok(PrepareSessionStart::Prepared(Box::new(record)))
            }
            RegistrationInsert::Existing(existing) => {
                self.require_record_context(&existing)?;
                self.require_prepared_identity(&existing, identity)?;
                if !matches_intent(&existing, intent) {
                    return Err(RegistrarFailure::OperationConflict);
                }
                Ok(PrepareSessionStart::Existing(existing))
            }
        }
    }

    /// A retained prepared row carries the trusted starter and immutable F1 facts.
    /// This is an internal harness handoff, never a session or capability lookup tool.
    pub async fn prepare_harness_start(
        &self,
        intent: &HarnessSessionLaunch,
        identity: &HarnessLaunchIdentity,
    ) -> Result<PrepareSessionStart, RegistrarFailure> {
        if self.mode.mode() == SessionRegistrationMode::AccessEnrollment {
            return Ok(PrepareSessionStart::AccessEnrollment);
        }
        let Some(record) = self.repository.operation(&intent.operation_id).await? else {
            let mut operator_launch = intent.clone();
            // Permission-routing metadata cannot supply starter authority without a prepared handoff.
            operator_launch.started_by = fleet_identity_protocol::StartedBy::RouterOperator {};
            return self.prepare(&operator_launch, identity).await;
        };
        self.require_record_context(&record)?;
        if !matches!(
            record.lifecycle(),
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {}
            }
        ) {
            return Err(RegistrarFailure::InvalidTransition);
        }
        self.require_prepared_identity(&record, identity)?;
        let retained = &record.preparation().launch;
        if retained.endpoint != intent.endpoint
            || Some(&retained.project_id) != intent.project_id.as_ref()
        {
            return Err(RegistrarFailure::OperationConflict);
        }
        Ok(PrepareSessionStart::Existing(Box::new(record)))
    }

    fn require_prepared_identity(
        &self,
        record: &FleetSessionRegistrationRecord,
        identity: &HarnessLaunchIdentity,
    ) -> Result<(), RegistrarFailure> {
        if matches!(
            record.lifecycle(),
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {}
            }
        ) && self
            .prepared_identities
            .lock()
            .map_err(|_| RegistrarFailure::Unavailable)?
            .get(&record.preparation().operation_id)
            != Some(identity)
        {
            return Err(RegistrarFailure::OperationConflict);
        }
        Ok(())
    }

    pub(super) fn forget_prepared_identity(
        &self,
        operation: &OperationId,
    ) -> Result<(), RegistrarFailure> {
        self.prepared_identities
            .lock()
            .map_err(|_| RegistrarFailure::Unavailable)?
            .remove(operation);
        Ok(())
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
            != *self.stable_origin()?
        {
            return Err(RegistrarFailure::OriginChanged);
        }
        Ok(())
    }
    pub(super) fn stable_origin(&self) -> Result<&SessionBindingOrigin, RegistrarFailure> {
        match &self.listener {
            SessionBindingListener::Stable(origin) => Ok(origin),
            SessionBindingListener::Ephemeral => Err(RegistrarFailure::EphemeralListener),
        }
    }

    #[must_use]
    pub fn registration_mode(&self) -> SessionRegistrationMode {
        self.mode.mode()
    }
}

fn matches_intent(record: &FleetSessionRegistrationRecord, intent: &HarnessSessionLaunch) -> bool {
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
    bytes
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
