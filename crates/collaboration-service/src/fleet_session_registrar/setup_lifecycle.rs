//! A real returned identity is retained before local setup can fail or be interrupted.
use super::{FleetSessionRegistrar, RegistrarFailure, RegistrationCompletion};
use codex_router_state::fleet_session_registration::{
    AbandonedSessionCreation, FleetSessionBindingLifecycle, PreparedSessionCreation,
};
use collaboration_protocol::{OperationId, SessionSetupAbandonmentCause};
use message_board::SessionRef;

impl FleetSessionRegistrar {
    pub async fn record_provider_created(
        &self,
        operation: &OperationId,
        session: SessionRef,
    ) -> Result<(), RegistrarFailure> {
        let current = self.record(operation).await?;
        match current.lifecycle() {
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Dispatched {},
            } => {
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
                        creation: PreparedSessionCreation::CreatedPendingSetup { session },
                    })
                    .map_err(|_| RegistrarFailure::OperationConflict)?;
                self.replace(&current, &next).await
            }
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session: existing },
            } if existing == &session => Ok(()),
            _ => Err(RegistrarFailure::InvalidTransition),
        }
    }

    pub async fn complete_created_setup(
        &self,
        operation: &OperationId,
    ) -> Result<RegistrationCompletion, RegistrarFailure> {
        let current = self.record(operation).await?;
        match current.lifecycle() {
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session },
            } => self.created(operation, session.clone()).await,
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Created { .. },
            }
            | FleetSessionBindingLifecycle::Active { .. } => {
                self.reconcile_registration(operation).await
            }
            _ => Err(RegistrarFailure::InvalidTransition),
        }
    }

    pub async fn abandon_created_setup(
        &self,
        operation: &OperationId,
        cause: SessionSetupAbandonmentCause,
    ) -> Result<(), RegistrarFailure> {
        let current = self.record(operation).await?;
        match current.lifecycle() {
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session },
            } => {
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Abandoned {
                        creation: AbandonedSessionCreation::KnownCreated {
                            session: session.clone(),
                            cause,
                        },
                    })
                    .map_err(|_| RegistrarFailure::InvalidTransition)?;
                self.replace(&current, &next).await
            }
            FleetSessionBindingLifecycle::Abandoned {
                creation:
                    AbandonedSessionCreation::KnownCreated {
                        cause: previous, ..
                    },
            } if previous == &cause => Ok(()),
            _ => Err(RegistrarFailure::InvalidTransition),
        }
    }

    /// Call once at startup before admitting new launches, never on a live connection refresh.
    /// It settles interrupted setup locally without any registration, prompt or provider RPC.
    pub async fn settle_interrupted_setups_after_restart(&self) -> Result<(), RegistrarFailure> {
        for record in self.repository.reconcile_candidates().await? {
            self.require_record_context(&record)?;
            if matches!(
                record.lifecycle(),
                FleetSessionBindingLifecycle::Prepared {
                    creation: PreparedSessionCreation::CreatedPendingSetup { .. }
                }
            ) {
                self.abandon_created_setup(
                    &record.preparation().operation_id,
                    SessionSetupAbandonmentCause::SetupInterrupted {},
                )
                .await?;
            }
        }
        Ok(())
    }
}
