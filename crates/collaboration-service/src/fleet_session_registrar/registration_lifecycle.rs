//! Provider outcomes and confirmed registration determine activation; an unknown create is never replayed.
use codex_router_state::fleet_session_registration::{
    AbandonedSessionCreation, FleetSessionBindingLifecycle, FleetSessionRegistrationRecord,
    PreparedSessionCreation, RegisteredFleetSession, SessionBindingAddress,
};
use collaboration_protocol::OperationId;
use fleet_identity_protocol::{
    EndFleetSession, RegisterFleetSessionResult, SessionEndCause, SessionRefusal,
};
use message_board::SessionRef;

use super::{BoundFleetSession, FleetSessionRegistrar, RegistrarFailure};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationCompletion {
    Registered(BoundFleetSession),
    Refused(SessionRefusal),
    Pending,
}

impl FleetSessionRegistrar {
    /// Confirm dispatch evidence before the provider's create; repeated calls cannot authorize another create.
    pub async fn mark_dispatched(
        &self,
        operation: &OperationId,
    ) -> Result<SessionBindingAddress, RegistrarFailure> {
        let current = self.record(operation).await?;
        if !matches!(
            current.lifecycle(),
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {}
            }
        ) {
            return Err(RegistrarFailure::InvalidTransition);
        }
        let next = current
            .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Dispatched {},
            })
            .map_err(|_| RegistrarFailure::InvalidTransition)?;
        self.replace(&current, &next).await?;
        self.forget_prepared_identity(operation)?;
        Ok(next.preparation().binding_address.clone())
    }

    /// Only a known provider refusal abandons a create; an ambiguous result remains dispatched/prepared.
    pub async fn creation_refused(&self, operation: &OperationId) -> Result<(), RegistrarFailure> {
        let current = self.record(operation).await?;
        if !matches!(
            current.lifecycle(),
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {}
                    | PreparedSessionCreation::Dispatched {}
            }
        ) {
            return Err(RegistrarFailure::InvalidTransition);
        }
        let next = current
            .with_lifecycle(FleetSessionBindingLifecycle::Abandoned {
                creation: AbandonedSessionCreation::NotCreated {},
            })
            .map_err(|_| RegistrarFailure::InvalidTransition)?;
        self.replace(&current, &next).await?;
        self.forget_prepared_identity(operation)
    }

    /// Records the actual provider identity and immutable request before any registration I/O.
    pub async fn created(
        &self,
        operation: &OperationId,
        session: SessionRef,
    ) -> Result<RegistrationCompletion, RegistrarFailure> {
        let current = self.record(operation).await?;
        let request = current.preparation().launch.registration(session);
        match current.lifecycle() {
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Dispatched {},
            } => {
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
                        creation: PreparedSessionCreation::Created {
                            registration: Box::new(request),
                        },
                    })
                    .map_err(|_| RegistrarFailure::OperationConflict)?;
                self.replace(&current, &next).await?;
            }
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::CreatedPendingSetup { session },
            } if session == &request.session => {
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
                        creation: PreparedSessionCreation::Created {
                            registration: Box::new(request),
                        },
                    })
                    .map_err(|_| RegistrarFailure::OperationConflict)?;
                self.replace(&current, &next).await?;
            }
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Created { registration },
            } if registration.as_ref() == &request => {}
            FleetSessionBindingLifecycle::Active { confirmed }
                if confirmed.registration == request =>
            {
                return BoundFleetSession::from_active(&current)
                    .map(RegistrationCompletion::Registered);
            }
            _ => return Err(RegistrarFailure::InvalidTransition),
        }
        self.reconcile_registration(operation).await
    }

    /// Reuses the exact persisted request after an ambiguous reply or restart, and never calls a provider.
    pub async fn reconcile_registration(
        &self,
        operation: &OperationId,
    ) -> Result<RegistrationCompletion, RegistrarFailure> {
        let current = self.record(operation).await?;
        let registration = match current.lifecycle() {
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::Created { registration },
            } => registration.as_ref(),
            FleetSessionBindingLifecycle::Active { confirmed } => &confirmed.registration,
            FleetSessionBindingLifecycle::Prepared { .. } => {
                return Ok(RegistrationCompletion::Pending);
            }
            FleetSessionBindingLifecycle::Abandoned { .. }
            | FleetSessionBindingLifecycle::Retired { .. } => {
                return Err(RegistrarFailure::InactiveBinding);
            }
        };
        let answer = match self.connection.register(registration).await {
            Ok(answer) => answer,
            Err(_) => return Ok(RegistrationCompletion::Pending),
        };
        match answer {
            RegisterFleetSessionResult::Registered {
                organization_id,
                workspace_id,
            } => {
                let confirmed = RegisteredFleetSession {
                    registration: registration.clone(),
                    organization_id,
                    workspace_id,
                };
                if let FleetSessionBindingLifecycle::Active {
                    confirmed: previous,
                } = current.lifecycle()
                {
                    if previous != &confirmed {
                        return Err(RegistrarFailure::OperationConflict);
                    }
                    return BoundFleetSession::from_active(&current)
                        .map(RegistrationCompletion::Registered);
                }
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Active { confirmed })
                    .map_err(|_| RegistrarFailure::OperationConflict)?;
                self.replace(&current, &next).await?;
                BoundFleetSession::from_active(&next).map(RegistrationCompletion::Registered)
            }
            RegisterFleetSessionResult::Refused { reason } => {
                let lifecycle = match current.lifecycle() {
                    FleetSessionBindingLifecycle::Active { confirmed } => {
                        FleetSessionBindingLifecycle::Retired {
                            confirmed: confirmed.clone(),
                        }
                    }
                    _ => FleetSessionBindingLifecycle::Abandoned {
                        creation: AbandonedSessionCreation::RegistrationRefused {
                            registration: Box::new(registration.clone()),
                        },
                    },
                };
                let next = current
                    .with_lifecycle(lifecycle)
                    .map_err(|_| RegistrarFailure::InvalidTransition)?;
                self.replace(&current, &next).await?;
                Ok(RegistrationCompletion::Refused(reason))
            }
        }
    }

    /// Local binding retirement precedes the authority report, including when its reply is unavailable.
    pub async fn end(
        &self,
        session: &SessionRef,
        cause: SessionEndCause,
    ) -> Result<(), RegistrarFailure> {
        let current = self
            .repository
            .conversation(session)
            .await?
            .ok_or(RegistrarFailure::InactiveBinding)?;
        self.require_record_context(&current)?;
        let confirmed = match current.lifecycle() {
            FleetSessionBindingLifecycle::Active { confirmed } => {
                let next = current
                    .with_lifecycle(FleetSessionBindingLifecycle::Retired {
                        confirmed: confirmed.clone(),
                    })
                    .map_err(|_| RegistrarFailure::InvalidTransition)?;
                self.replace(&current, &next).await?;
                confirmed
            }
            FleetSessionBindingLifecycle::Retired { confirmed } => confirmed,
            _ => return Err(RegistrarFailure::InactiveBinding),
        };
        self.connection
            .end(&EndFleetSession {
                organization_id: confirmed.organization_id.clone(),
                session: session.clone(),
                cause,
            })
            .await?;
        Ok(())
    }

    pub(super) async fn record(
        &self,
        operation: &OperationId,
    ) -> Result<FleetSessionRegistrationRecord, RegistrarFailure> {
        let record = self
            .repository
            .operation(operation)
            .await?
            .ok_or(RegistrarFailure::InvalidTransition)?;
        self.require_record_context(&record)?;
        Ok(record)
    }
    pub(super) async fn replace(
        &self,
        current: &FleetSessionRegistrationRecord,
        next: &FleetSessionRegistrationRecord,
    ) -> Result<(), RegistrarFailure> {
        if !self.repository.replace(current, next).await? {
            return Err(RegistrarFailure::TransitionConflict);
        }
        Ok(())
    }
}
