//! One immutable enrollment cache and the existing session resolve mail recipients.
use super::{MailDeliveryRuntime, MailDeliverySession};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardReadPage, BoardReadScope,
    InstallationPeerLookup,
};
use durable_steps::StoreError;
use fleet_sync_protocol::{OperationAnswer, OperationRequest, PeerId};
use interaction_storage::{InstallationPeerBinding, StorageError};
use message_board::ServiceId;

#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallationPeerResolutionError {
    #[error("recipient installation is not enrolled")]
    RecipientUnknown,
    #[error("recipient installation lookup is unavailable")]
    Unavailable,
    #[error("installation directory is corrupt: {field}")]
    Corrupt { field: &'static str },
}

impl MailDeliveryRuntime {
    /// Read every enrolled-installation page into the persistent local cache.
    pub async fn refresh_installation_peers(&self) -> Result<(), StoreError> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| StoreError::backend("installation directory session is not attached"))?;
        let mut after = None;
        loop {
            if !*session.welcomed.borrow() {
                return Err(StoreError::backend(
                    "installation directory session is not welcomed",
                ));
            }
            let answer = session
                .requests
                .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope: BoardReadScope::InstallationPeers { after },
                    cursor: None,
                }))
                .await
                .map_err(StoreError::backend)?;
            let OperationAnswer::Consumer(BoardConsumerAnswer::Read(
                BoardReadPage::InstallationPeers(page),
            )) = answer
            else {
                return Err(StoreError::backend(
                    "installation directory returned another read family",
                ));
            };
            page.validate_after(after).map_err(StoreError::backend)?;
            let bindings = page
                .peers()
                .iter()
                .map(|(service, peer)| {
                    Ok(InstallationPeerBinding {
                        service_id: service.clone(),
                        peer_id: peer
                            .as_uuid()
                            .to_string()
                            .try_into()
                            .map_err(StoreError::backend)?,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            self.owner
                .remember_installation_peers(&bindings)
                .await
                .map_err(StoreError::backend)?;
            after = page.next();
            if after.is_none() {
                return Ok(());
            }
        }
    }

    pub(crate) async fn cached_installation_peer(
        &self,
        service: &ServiceId,
    ) -> Result<Option<PeerId>, InstallationPeerResolutionError> {
        if service == &self.service {
            return Ok(Some(self.config.owner));
        }
        self.owner
            .installation_peer(service)
            .await
            .map_err(directory_storage_error)?
            .map(|peer| {
                let invalid = || InstallationPeerResolutionError::Corrupt {
                    field: "cached peer identity",
                };
                PeerId::new(uuid::Uuid::parse_str(peer.as_str()).map_err(|_| invalid())?)
                    .map_err(|_| invalid())
            })
            .transpose()
    }

    pub(crate) async fn resolve_installation_peer(
        &self,
        service: &ServiceId,
    ) -> Result<PeerId, InstallationPeerResolutionError> {
        if let Some(peer) = self.cached_installation_peer(service).await? {
            return Ok(peer);
        }
        let session = self
            .session
            .as_ref()
            .ok_or(InstallationPeerResolutionError::Unavailable)?;
        match read_installation_peer(session, service).await? {
            InstallationPeerLookup::Unknown => {
                Err(InstallationPeerResolutionError::RecipientUnknown)
            }
            InstallationPeerLookup::Enrolled { peer } => {
                self.owner
                    .remember_installation_peer(&InstallationPeerBinding {
                        service_id: service.clone(),
                        peer_id: peer.as_uuid().to_string().try_into().map_err(|_| {
                            InstallationPeerResolutionError::Corrupt {
                                field: "enrolled peer identity",
                            }
                        })?,
                    })
                    .await
                    .map_err(directory_storage_error)?;
                Ok(peer)
            }
        }
    }
}

async fn read_installation_peer(
    session: &MailDeliverySession,
    service: &ServiceId,
) -> Result<InstallationPeerLookup, InstallationPeerResolutionError> {
    if !*session.welcomed.borrow() {
        return Err(InstallationPeerResolutionError::Unavailable);
    }
    match session
        .requests
        .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::InstallationPeer {
                service: service.clone(),
            },
            cursor: None,
        }))
        .await
    {
        Ok(OperationAnswer::Consumer(BoardConsumerAnswer::Read(
            BoardReadPage::InstallationPeer(answer),
        ))) => Ok(answer),
        Ok(_) | Err(_) => Err(InstallationPeerResolutionError::Unavailable),
    }
}

fn directory_storage_error(error: StorageError) -> InstallationPeerResolutionError {
    match error {
        StorageError::InvalidStoredRecord | StorageError::InstallationPeerConflict => {
            InstallationPeerResolutionError::Corrupt {
                field: "stored enrollment binding",
            }
        }
        _ => InstallationPeerResolutionError::Unavailable,
    }
}
