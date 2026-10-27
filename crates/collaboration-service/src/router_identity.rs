//! Immutable validated facts identifying one Router installation and run.
use collaboration_contracts::MachineIdentity;
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;

#[derive(Clone, Debug)]
pub struct RouterIdentity {
    service_id: UuidIdentity,
    service_epoch: UuidIdentity,
    installation_peer: Option<PeerId>,
    machine_identity: MachineIdentity,
}
impl RouterIdentity {
    pub fn new(
        service_id: UuidIdentity,
        service_epoch: UuidIdentity,
        installation_peer: Option<PeerId>,
        machine_identity: MachineIdentity,
    ) -> Result<Self, RouterIdentityError> {
        if machine_identity.service_id() != &service_id {
            return Err(RouterIdentityError::MachineServiceMismatch);
        }
        Ok(Self {
            service_id,
            service_epoch,
            installation_peer,
            machine_identity,
        })
    }
    #[must_use]
    pub fn service_id(&self) -> &UuidIdentity {
        &self.service_id
    }
    #[must_use]
    pub fn service_epoch(&self) -> &UuidIdentity {
        &self.service_epoch
    }
    #[must_use]
    pub fn installation_peer(&self) -> Option<PeerId> {
        self.installation_peer
    }
    #[must_use]
    pub fn machine_identity(&self) -> &MachineIdentity {
        &self.machine_identity
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RouterIdentityError {
    #[error("machine identity belongs to another service")]
    MachineServiceMismatch,
}

#[cfg(test)]
mod tests {
    use super::{RouterIdentity, RouterIdentityError};
    use collaboration_contracts::MachineIdentity;
    use collaboration_protocol::UuidIdentity;
    use fleet_sync_protocol::PeerId;

    #[test]
    fn router_identity_rejects_a_machine_from_another_service() {
        let service: UuidIdentity = "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .expect("service");
        let other: UuidIdentity = "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .expect("other service");
        let machine = MachineIdentity::new(other, Some("Other machine")).expect("machine");

        let result = RouterIdentity::new(service.clone(), service, None, machine);

        assert_eq!(
            result.expect_err("different service"),
            RouterIdentityError::MachineServiceMismatch
        );
    }

    #[test]
    fn cloned_router_identity_preserves_validated_installation_facts() {
        let service: UuidIdentity = "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .expect("service");
        let epoch: UuidIdentity = "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .expect("epoch");
        let peer = PeerId::new(
            uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000003").expect("peer UUID"),
        )
        .expect("peer");
        let machine =
            MachineIdentity::new(service.clone(), Some("Router machine")).expect("machine");
        let identity = RouterIdentity::new(service.clone(), epoch.clone(), Some(peer), machine)
            .expect("identity");

        let cloned = identity.clone();
        assert_eq!(identity.service_id(), &service);

        assert_eq!(cloned.service_id(), &service);
        assert_eq!(cloned.service_epoch(), &epoch);
        assert_eq!(cloned.installation_peer(), Some(peer));
        assert_eq!(
            cloned.machine_identity().machine_label().as_str(),
            "Router machine"
        );
    }
}
