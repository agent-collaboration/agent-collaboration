//! The Host's one machine session retains its library-owned resume state locally.
use fleet_sync_session::{ClientSessionState, SessionClientStore, StoreError};
use std::path::{Path, PathBuf};
use tokio::{io::AsyncWriteExt, sync::Mutex};

pub struct MachineSessionStore {
    directory: PathBuf,
    serial: Mutex<()>,
}

impl MachineSessionStore {
    /// The caller owns this Router directory and constructs one live session store.
    pub async fn open(router_directory: &Path) -> Result<Self, StoreError> {
        let directory = router_directory.join("machine-session");
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(unavailable)?;
        Ok(Self {
            directory,
            serial: Mutex::new(()),
        })
    }
}

impl SessionClientStore for MachineSessionStore {
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        let _serial = self.serial.lock().await;
        match tokio::fs::read(self.directory.join("state.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(corrupt_state),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(unavailable(error)),
        }
    }

    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        let _serial = self.serial.lock().await;
        let bytes = serde_json::to_vec(state).map_err(corrupt_state)?;
        let temporary = self
            .directory
            .join(format!("state.{}.tmp", uuid::Uuid::now_v7()));
        let result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&temporary, self.directory.join("state.json")).await?;
            tokio::fs::File::open(&self.directory)
                .await?
                .sync_all()
                .await?;
            Ok::<_, std::io::Error>(())
        }
        .await;
        if result.is_err() {
            // Only this operation created this uniquely named temporary file.
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result.map_err(unavailable)
    }
}

fn unavailable(error: std::io::Error) -> StoreError {
    StoreError::Unavailable {
        message: error.to_string(),
    }
}

fn corrupt_state(_: serde_json::Error) -> StoreError {
    StoreError::Corrupt {
        field: "machineSessionState".to_owned(),
    }
}
