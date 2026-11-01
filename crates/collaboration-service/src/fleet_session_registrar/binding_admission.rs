//! Only an active, local persisted registration resolves to a session actor.
use codex_router_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, FleetSessionRegistrationId, FleetSessionRegistrationRecord,
    SessionBindingAddress,
};
use fleet_identity_protocol::OrganizationId;
use message_board::{Identity, SessionRef, WorkspaceId};

use super::{FleetSessionRegistrar, RegistrarFailure};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundFleetSession {
    registration_id: FleetSessionRegistrationId,
    session: SessionRef,
    organization_id: OrganizationId,
    workspace_id: WorkspaceId,
}
impl BoundFleetSession {
    pub(super) fn from_active(
        record: &FleetSessionRegistrationRecord,
    ) -> Result<Self, RegistrarFailure> {
        let FleetSessionBindingLifecycle::Active { confirmed } = record.lifecycle() else {
            return Err(RegistrarFailure::InactiveBinding);
        };
        Ok(Self {
            registration_id: record.preparation().registration_id.clone(),
            session: confirmed.registration.session.clone(),
            organization_id: confirmed.organization_id.clone(),
            workspace_id: confirmed.workspace_id.clone(),
        })
    }
    #[must_use]
    pub fn registration_id(&self) -> &FleetSessionRegistrationId {
        &self.registration_id
    }
    #[must_use]
    pub fn session(&self) -> &SessionRef {
        &self.session
    }
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    #[must_use]
    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }
    pub fn require_sender(&self, claimed: &SessionRef) -> Result<(), RegistrarFailure> {
        if claimed == &self.session {
            Ok(())
        } else {
            Err(RegistrarFailure::SenderMismatch)
        }
    }
    pub fn require_actor(&self, claimed: &Identity) -> Result<(), RegistrarFailure> {
        match claimed {
            Identity::Session { session } => self.require_sender(session),
            Identity::Human { .. } => Err(RegistrarFailure::SenderMismatch),
        }
    }
}

impl FleetSessionRegistrar {
    /// Resolves the URI's private path segment against this installation's fixed origin.
    pub async fn resolve_binding_capability(
        &self,
        capability: &str,
    ) -> Result<BoundFleetSession, RegistrarFailure> {
        if capability.len() != 32 {
            return Err(RegistrarFailure::InactiveBinding);
        }
        let address = format!(
            "{}/mcp/session/{capability}",
            self.stable_origin()?.as_str()
        )
        .try_into()
        .map_err(|_| RegistrarFailure::InactiveBinding)?;
        self.resolve_binding(&address).await
    }

    pub async fn resolve_binding(
        &self,
        address: &SessionBindingAddress,
    ) -> Result<BoundFleetSession, RegistrarFailure> {
        let record = self
            .repository
            .binding(address)
            .await?
            .ok_or(RegistrarFailure::InactiveBinding)?;
        self.require_record_context(&record)?;
        match record.lifecycle() {
            FleetSessionBindingLifecycle::Active { confirmed } => Ok(BoundFleetSession {
                registration_id: record.preparation().registration_id.clone(),
                session: confirmed.registration.session.clone(),
                organization_id: confirmed.organization_id.clone(),
                workspace_id: confirmed.workspace_id.clone(),
            }),
            FleetSessionBindingLifecycle::Prepared { .. }
            | FleetSessionBindingLifecycle::Abandoned { .. }
            | FleetSessionBindingLifecycle::Retired { .. } => {
                Err(RegistrarFailure::InactiveBinding)
            }
        }
    }
    pub async fn binding_for_restore(
        &self,
        session: &SessionRef,
    ) -> Result<SessionBindingAddress, RegistrarFailure> {
        let record = self
            .repository
            .conversation(session)
            .await?
            .ok_or(RegistrarFailure::InactiveBinding)?;
        self.require_record_context(&record)?;
        if matches!(
            record.lifecycle(),
            FleetSessionBindingLifecycle::Active { .. }
        ) {
            Ok(record.preparation().binding_address.clone())
        } else {
            Err(RegistrarFailure::InactiveBinding)
        }
    }
}
