//! Owns the Router's one durable identity. A corrupt row is never replaced.
use crate::{MACHINE_LOCAL_DATABASE, MachineLocalStore, MachineLocalStoreError};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::TransactionStart;
use std::{io, os::unix::fs::PermissionsExt, path::Path};
use workspace_ids::{IdValidationError, MachineId};

#[derive(Debug, thiserror::Error)]
pub enum MachineIdentityError {
    #[error("machine identity directory must be private and absolute")]
    InvalidDirectory,
    #[error("cannot inspect the machine identity directory")]
    Directory(#[source] io::Error),
    #[error(transparent)]
    Store(#[from] MachineLocalStoreError),
    #[error("machine identity database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("machine_identity.machine_id is corrupt")]
    InvalidStoredId(#[source] IdValidationError),
    #[error("machine identity clock is outside the UUIDv7 timestamp range")]
    ClockRange,
    #[error("minted machine id is invalid")]
    InvalidMintedId(#[source] IdValidationError),
}

pub async fn load_or_create_machine_id(
    directory: &Path,
    now: SyncTime,
) -> Result<MachineId, MachineIdentityError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(MachineIdentityError::Directory)?;
    if !directory.is_absolute() || !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(MachineIdentityError::InvalidDirectory);
    }
    let mut store = MachineLocalStore::open(&directory.join(MACHINE_LOCAL_DATABASE)).await?;
    let loaded = store.load_or_create_machine_id(now).await;
    let closed = store.close().await;
    let machine_id = loaded?;
    closed?;
    Ok(machine_id)
}

impl MachineLocalStore {
    pub async fn load_or_create_machine_id(
        &mut self,
        now: SyncTime,
    ) -> Result<MachineId, MachineIdentityError> {
        let mut transaction = self.transaction(TransactionStart::Immediate).await?;
        let stored = sqlx::query_scalar!("SELECT machine_id FROM machine_identity")
            .fetch_optional(&mut *transaction)
            .await?;
        let machine_id = match stored {
            Some(stored) => stored
                .parse()
                .map_err(MachineIdentityError::InvalidStoredId)?,
            None => {
                let machine_id = mint_machine_id(now)?;
                sqlx::query("INSERT INTO machine_identity (machine_id, created_at, updated_at) VALUES (?, ?, ?)")
                    .bind(machine_id.as_str())
                    .bind(now.unix_millis())
                    .bind(now.unix_millis())
                    .execute(&mut *transaction).await?;
                machine_id
            }
        };
        transaction.commit().await?;
        Ok(machine_id)
    }
}

fn mint_machine_id(now: SyncTime) -> Result<MachineId, MachineIdentityError> {
    let millis = u64::try_from(now.unix_millis()).map_err(|_| MachineIdentityError::ClockRange)?;
    if millis >= (1_u64 << 48) {
        return Err(MachineIdentityError::ClockRange);
    }
    let nanos =
        u32::try_from(millis % 1_000).map_err(|_| MachineIdentityError::ClockRange)? * 1_000_000;
    let timestamp = uuid::Timestamp::from_unix(uuid::ContextV7::new(), millis / 1_000, nanos);
    uuid::Uuid::new_v7(timestamp)
        .to_string()
        .parse()
        .map_err(MachineIdentityError::InvalidMintedId)
}

#[cfg(test)]
#[path = "machine_identity_store_test.rs"]
mod tests;
