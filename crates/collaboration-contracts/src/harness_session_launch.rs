//! Harnesses consume launch decisions; the Host adapts the sole registrar policy owner.
use collaboration_protocol::{EndpointRef, OperationId, SessionSetupAbandonmentCause};
use fleet_identity_protocol::{
    AgentIsolation, LaunchContext, MachineEnvironment, SessionRefusal, SessionStartRefusal,
    StartedBy,
};
use message_board::{ProjectId, SessionRef};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessSessionLaunch {
    pub operation_id: OperationId,
    pub endpoint: EndpointRef,
    pub project_id: Option<ProjectId>,
    pub started_by: StartedBy,
    pub isolation: AgentIsolation,
    pub launch_context: LaunchContext,
    pub machine_environment: MachineEnvironment,
}

/// The Host supplies an address from its validated private registration. It is
/// intentionally opaque to serializers and diagnostics; only harness config uses it.
#[derive(Clone)]
pub struct HarnessSessionBinding {
    operation_id: OperationId,
    address: String,
}
impl HarnessSessionBinding {
    #[must_use]
    pub fn from_host_record(operation_id: OperationId, address: String) -> Self {
        Self {
            operation_id,
            address,
        }
    }
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }
    #[must_use]
    pub fn expose_address(&self) -> &str {
        &self.address
    }
}

pub enum HarnessStartAdmission {
    AccessEnrollment,
    Prepared(HarnessSessionBinding),
}
pub enum HarnessRestoreAdmission {
    AccessEnrollment,
    Bound(HarnessSessionBinding),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessRegistrationOutcome {
    Registered,
    Pending,
    Refused(SessionRefusal),
}

/// Only completed local checks report these causes. Startup owns setupInterrupted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessLocalSetupRefusal {
    ModelMismatch,
    EffectiveWorkingDirectoryMismatch,
    WorkspaceRootsMismatch,
}
impl From<HarnessLocalSetupRefusal> for SessionSetupAbandonmentCause {
    fn from(cause: HarnessLocalSetupRefusal) -> Self {
        match cause {
            HarnessLocalSetupRefusal::ModelMismatch => Self::ModelMismatch {},
            HarnessLocalSetupRefusal::EffectiveWorkingDirectoryMismatch => {
                Self::EffectiveWorkingDirectoryMismatch {}
            }
            HarnessLocalSetupRefusal::WorkspaceRootsMismatch => Self::WorkspaceRootsMismatch {},
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HarnessLaunchFailure {
    #[error("a session start must name its project")]
    ProjectRequired,
    #[error("the organization authority refused the start")]
    StartRefused(SessionStartRefusal),
    #[error("this launch operation already has retained state; it must not create again")]
    AlreadyPrepared,
    #[error("the session launch owner is unavailable")]
    Unavailable,
    #[error("the session binding is inactive or belongs to another installation or origin")]
    InactiveBinding,
    #[error("the retained launch cannot make this transition")]
    InvalidTransition,
}
pub type HarnessLaunchFuture<'a, TOutcome> =
    Pin<Box<dyn Future<Output = Result<TOutcome, HarnessLaunchFailure>> + Send + 'a>>;

pub trait HarnessSessionLaunchPort: Send + Sync {
    fn prepare_start<'a>(
        &'a self,
        launch: &'a HarnessSessionLaunch,
    ) -> HarnessLaunchFuture<'a, HarnessStartAdmission>;
    /// Must finish before the native/provider create RPC can be dispatched.
    fn mark_dispatched<'a>(&'a self, operation: &'a OperationId) -> HarnessLaunchFuture<'a, ()>;
    /// Must finish immediately after a valid returned ID, before post-create checks.
    fn record_created<'a>(
        &'a self,
        operation: &'a OperationId,
        session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, ()>;
    fn complete_setup<'a>(
        &'a self,
        operation: &'a OperationId,
    ) -> HarnessLaunchFuture<'a, HarnessRegistrationOutcome>;
    fn refuse_setup<'a>(
        &'a self,
        operation: &'a OperationId,
        cause: HarnessLocalSetupRefusal,
    ) -> HarnessLaunchFuture<'a, ()>;
    /// Restoring uses an active persisted binding and never re-registers the conversation.
    fn binding_for_restore<'a>(
        &'a self,
        session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, HarnessRestoreAdmission>;
}
