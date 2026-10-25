//! Authenticated enrollment bindings only accumulate; cached pairs never change or expire.
use crate::{InteractionStore, StorageError};
use collaboration_protocol::InstallationPeerId;
use message_board::ServiceId;
use sqlx::Connection;
use sqlx_turso::TursoConnection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationPeerBinding {
    pub service_id: ServiceId,
    pub peer_id: InstallationPeerId,
}

struct InstallationPeerRow {
    service_id: String,
    peer_id: String,
}

impl TryFrom<InstallationPeerRow> for InstallationPeerBinding {
    type Error = StorageError;
    fn try_from(row: InstallationPeerRow) -> Result<Self, Self::Error> {
        Ok(Self {
            service_id: row
                .service_id
                .try_into()
                .map_err(|_| StorageError::InvalidStoredRecord)?,
            peer_id: row
                .peer_id
                .try_into()
                .map_err(|_| StorageError::InvalidStoredRecord)?,
        })
    }
}

async fn remember_binding(
    connection: &mut TursoConnection,
    binding: &InstallationPeerBinding,
) -> Result<(), StorageError> {
    // Either unique key may already exist. Read both afterward, so a conflict
    // cannot overwrite an immutable pair or conceal malformed stored identities.
    sqlx_turso::query!(
        "INSERT INTO installation_peer_directory(peer_id,service_id) VALUES (?,?) ON CONFLICT DO NOTHING",
        binding.peer_id.as_str(), binding.service_id.as_str()
    ).execute(&mut *connection).await?;
    let rows = sqlx_turso::query_as!(InstallationPeerRow,
        "SELECT service_id AS \"service_id!: String\",peer_id AS \"peer_id!: String\" FROM installation_peer_directory WHERE service_id=? OR peer_id=?",
        binding.service_id.as_str(), binding.peer_id.as_str()
    ).fetch_all(connection).await?;
    let stored = rows
        .into_iter()
        .map(InstallationPeerBinding::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    if stored.is_empty() {
        return Err(StorageError::InvalidStoredRecord);
    }
    if stored.len() != 1 || stored.first() != Some(binding) {
        return Err(StorageError::InstallationPeerConflict);
    }
    Ok(())
}

impl InteractionStore {
    /// Read before any live lookup. Malformed matching rows refuse instead of becoming misses.
    pub async fn installation_peer(
        &self,
        service: &ServiceId,
    ) -> Result<Option<InstallationPeerId>, StorageError> {
        let row = sqlx_turso::query_as!(InstallationPeerRow,
            "SELECT service_id AS \"service_id!: String\",peer_id AS \"peer_id!: String\" FROM installation_peer_directory WHERE service_id=?",
            service.as_str()
        ).fetch_optional(&mut *self.writer.lock().await).await?;
        row.map(InstallationPeerBinding::try_from)
            .transpose()
            .map(|binding| binding.map(|binding| binding.peer_id))
    }

    pub async fn remember_installation_peer(
        &self,
        binding: &InstallationPeerBinding,
    ) -> Result<(), StorageError> {
        self.remember_installation_peers(std::slice::from_ref(binding))
            .await
    }

    /// One page inserts atomically. Repeated pairs are harmless; either key's reassignment refuses.
    pub async fn remember_installation_peers(
        &self,
        bindings: &[InstallationPeerBinding],
    ) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            for binding in bindings {
                remember_binding(&mut transaction, binding).await?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "installation_peer_directory_tests.rs"]
mod tests;
