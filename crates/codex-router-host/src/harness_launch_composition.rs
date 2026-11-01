//! The Host translates the shared harness port into the sole registrar's policy.
use codex_router_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, PreparedSessionCreation,
};
use collaboration_contracts::{
    HarnessLaunchFailure, HarnessLaunchFuture, HarnessLocalSetupRefusal,
    HarnessRegistrationOutcome, HarnessRestoreAdmission, HarnessSessionBinding,
    HarnessSessionLaunch, HarnessSessionLaunchPort, HarnessStartAdmission,
};
use collaboration_protocol::{NonEmptyText, OperationId};
use collaboration_service::fleet_session_registrar::{
    FleetSessionRegistrar, PrepareSessionStart, RegistrarFailure, RegistrationCompletion,
    SessionRegistrationMode,
};
use message_board::SessionRef;
use std::sync::Arc;

const NATIVE_COLLABORATION_MCP_SERVER_NAME: &str = "agent-router";

pub struct HostHarnessSessionLaunchPort {
    registrar: Arc<FleetSessionRegistrar>,
}
impl HostHarnessSessionLaunchPort {
    #[must_use]
    pub fn new(registrar: Arc<FleetSessionRegistrar>) -> Self {
        Self { registrar }
    }

    /// Finish before opening launch admission; reconnecting a live machine is not a restart.
    pub async fn settle_interrupted_setups_after_restart(
        &self,
    ) -> Result<(), HarnessLaunchFailure> {
        self.registrar
            .settle_interrupted_setups_after_restart()
            .await
            .map_err(launch_failure)
    }
}

fn binding(address: &str) -> Result<HarnessSessionBinding, HarnessLaunchFailure> {
    let name = NonEmptyText::try_from(NATIVE_COLLABORATION_MCP_SERVER_NAME.to_owned())
        .map_err(|_| HarnessLaunchFailure::Unavailable)?;
    Ok(HarnessSessionBinding::from_host_record(
        name,
        address.to_owned(),
    ))
}

impl HarnessSessionLaunchPort for HostHarnessSessionLaunchPort {
    fn prepare_start<'a>(
        &'a self,
        launch: &'a HarnessSessionLaunch,
    ) -> HarnessLaunchFuture<'a, HarnessStartAdmission> {
        Box::pin(async move {
            match self
                .registrar
                .prepare(launch)
                .await
                .map_err(launch_failure)?
            {
                PrepareSessionStart::AccessEnrollment => {
                    Ok(HarnessStartAdmission::AccessEnrollment)
                }
                PrepareSessionStart::Prepared(record) => Ok(HarnessStartAdmission::Prepared(
                    binding(record.preparation().binding_address.expose_address())?,
                )),
                PrepareSessionStart::Existing(record) => {
                    if !matches!(
                        record.lifecycle(),
                        FleetSessionBindingLifecycle::Prepared {
                            creation: PreparedSessionCreation::NotDispatched {}
                        }
                    ) {
                        return Err(HarnessLaunchFailure::AlreadyPrepared);
                    }
                    // mark_dispatched persists consumption before create and refuses a second consumer.
                    Ok(HarnessStartAdmission::Prepared(binding(
                        record.preparation().binding_address.expose_address(),
                    )?))
                }
            }
        })
    }
    fn mark_dispatched<'a>(&'a self, operation: &'a OperationId) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move {
            self.registrar
                .mark_dispatched(operation)
                .await
                .map(|_| ())
                .map_err(launch_failure)
        })
    }
    fn record_created<'a>(
        &'a self,
        operation: &'a OperationId,
        session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move {
            self.registrar
                .record_provider_created(operation, session.clone())
                .await
                .map_err(launch_failure)
        })
    }
    fn complete_setup<'a>(
        &'a self,
        operation: &'a OperationId,
    ) -> HarnessLaunchFuture<'a, HarnessRegistrationOutcome> {
        Box::pin(async move {
            Ok(
                match self
                    .registrar
                    .complete_created_setup(operation)
                    .await
                    .map_err(launch_failure)?
                {
                    RegistrationCompletion::Registered(_) => HarnessRegistrationOutcome::Registered,
                    RegistrationCompletion::Pending => HarnessRegistrationOutcome::Pending,
                    RegistrationCompletion::Refused(reason) => {
                        HarnessRegistrationOutcome::Refused(reason)
                    }
                },
            )
        })
    }
    fn refuse_setup<'a>(
        &'a self,
        operation: &'a OperationId,
        cause: HarnessLocalSetupRefusal,
    ) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move {
            self.registrar
                .abandon_created_setup(operation, cause.into())
                .await
                .map_err(launch_failure)
        })
    }
    fn binding_for_restore<'a>(
        &'a self,
        session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, HarnessRestoreAdmission> {
        Box::pin(async move {
            if self.registrar.registration_mode() == SessionRegistrationMode::AccessEnrollment {
                return Ok(HarnessRestoreAdmission::AccessEnrollment);
            }
            let address = self
                .registrar
                .binding_for_restore(session)
                .await
                .map_err(launch_failure)?;
            Ok(HarnessRestoreAdmission::Bound(binding(
                address.expose_address(),
            )?))
        })
    }
}

fn launch_failure(error: RegistrarFailure) -> HarnessLaunchFailure {
    match error {
        RegistrarFailure::ProjectRequired => HarnessLaunchFailure::ProjectRequired,
        RegistrarFailure::StartRefused(reason) => HarnessLaunchFailure::StartRefused(reason),
        RegistrarFailure::ForeignInstallation
        | RegistrarFailure::OriginChanged
        | RegistrarFailure::EphemeralListener
        | RegistrarFailure::InactiveBinding
        | RegistrarFailure::SenderMismatch => HarnessLaunchFailure::InactiveBinding,
        RegistrarFailure::OperationConflict
        | RegistrarFailure::InvalidTransition
        | RegistrarFailure::TransitionConflict => HarnessLaunchFailure::InvalidTransition,
        RegistrarFailure::Unavailable
        | RegistrarFailure::Connection(_)
        | RegistrarFailure::Storage(_) => HarnessLaunchFailure::Unavailable,
    }
}
