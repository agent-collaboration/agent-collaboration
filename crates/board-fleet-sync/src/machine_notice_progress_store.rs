//! Router-owned machine progress is independent of the library's session transport state.
use fleet_sync_protocol::NoticePosition;
use fleet_sync_session::StoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::{io::AsyncWriteExt, sync::Mutex};

pub struct MachineNoticeProgressStore {
    directory: PathBuf,
    serial: Mutex<()>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MachineNoticeProgress {
    handled_through: NoticePosition,
}

impl MachineNoticeProgressStore {
    /// The Host constructs one live progress owner beside its separate session store.
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

    pub async fn load(&self) -> Result<NoticePosition, StoreError> {
        let _serial = self.serial.lock().await;
        match tokio::fs::read(self.directory.join("machine-notice-progress.json")).await {
            Ok(bytes) => serde_json::from_slice::<MachineNoticeProgress>(&bytes)
                .map(|progress| progress.handled_through)
                .map_err(corrupt_progress),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(NoticePosition::new(0))
            }
            Err(error) => Err(unavailable(error)),
        }
    }

    /// Save only after successful idempotent handling; recovery may explicitly reset the position.
    pub async fn save(&self, handled_through: NoticePosition) -> Result<(), StoreError> {
        let _serial = self.serial.lock().await;
        let bytes = serde_json::to_vec(&MachineNoticeProgress { handled_through })
            .map_err(corrupt_progress)?;
        let temporary = self.directory.join(format!(
            "machine-notice-progress.{}.tmp",
            uuid::Uuid::now_v7()
        ));
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
            tokio::fs::rename(
                &temporary,
                self.directory.join("machine-notice-progress.json"),
            )
            .await?;
            tokio::fs::File::open(&self.directory)
                .await?
                .sync_all()
                .await?;
            Ok::<_, std::io::Error>(())
        }
        .await;
        if result.is_err() {
            // This save alone created this uniquely named file.
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
fn corrupt_progress(_: serde_json::Error) -> StoreError {
    StoreError::Corrupt {
        field: "machineNoticeProgress".to_owned(),
    }
}
