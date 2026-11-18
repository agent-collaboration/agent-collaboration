//! The wire names a Router installation. A native route names its local service.
use collaboration_protocol::{EndpointRef, SessionRef, UuidIdentity};
use workspace_model::{MachineId, SessionAddress};

#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallationAddressError {
    #[error("invalid endpoint in the session address")]
    Endpoint,
    #[error("invalid opaque session id in the session address")]
    Session,
}

#[derive(Clone)]
pub(crate) struct InstallationAddress {
    installation_id: MachineId,
    service_id: UuidIdentity,
}

impl InstallationAddress {
    pub(crate) const fn new(installation_id: MachineId, service_id: UuidIdentity) -> Self {
        Self {
            installation_id,
            service_id,
        }
    }

    pub(crate) fn local_session_ref(
        &self,
        address: &SessionAddress,
    ) -> Result<Option<SessionRef>, InstallationAddressError> {
        if address.machine_id != self.installation_id {
            return Ok(None);
        }
        Ok(Some(SessionRef {
            endpoint: EndpointRef {
                service_id: self.service_id.clone(),
                endpoint_id: address
                    .endpoint
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| InstallationAddressError::Endpoint)?,
            },
            session_id: address
                .session_id
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| InstallationAddressError::Session)?,
        }))
    }

    pub(crate) fn local_session_address(
        &self,
        session: &SessionRef,
    ) -> Result<Option<SessionAddress>, InstallationAddressError> {
        if session.endpoint.service_id != self.service_id {
            return Ok(None);
        }
        Ok(Some(SessionAddress {
            machine_id: self.installation_id.clone(),
            endpoint: String::from(session.endpoint.endpoint_id.clone())
                .try_into()
                .map_err(|_| InstallationAddressError::Endpoint)?,
            session_id: String::from(session.session_id.clone())
                .try_into()
                .map_err(|_| InstallationAddressError::Session)?,
        }))
    }
}

#[cfg(test)]
#[path = "installation_address_test.rs"]
mod tests;
