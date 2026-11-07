//! Commit validated legacy state before deleting any source file; never overwrite a committed row.
use super::{
    ClientSessionState, DeserializeOwned, FleetCache, MachineNoticeProgress,
    RouterLocalStorageError, RuntimeStateKind, SnapshotInstallAttempt, insert_attempt,
    read_runtime, write_runtime,
};
use std::path::{Path, PathBuf};

impl FleetCache {
    pub async fn import_legacy_runtime(
        &self,
        directory: &Path,
    ) -> Result<(), RouterLocalStorageError> {
        let session_path = directory.join("machine-session/state.json");
        let progress_path = directory.join("machine-session/machine-notice-progress.json");
        let session: Option<ClientSessionState> =
            read_legacy(&session_path, "machineSessionState").await?;
        let progress: Option<MachineNoticeProgress> =
            read_legacy(&progress_path, "machineNoticeProgress").await?;
        let mut attempts = Vec::new();
        match tokio::fs::read_dir(directory.join("snapshot-install-attempts")).await {
            Ok(mut entries) => {
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    if path.extension().is_none_or(|extension| extension != "json") {
                        continue;
                    }
                    let record: SnapshotInstallAttempt =
                        read_legacy(&path, "snapshotInstallAttempt").await?.ok_or(
                            RouterLocalStorageError::InvalidRecordKey {
                                field: "snapshotInstallAttempt",
                            },
                        )?;
                    if path.file_name().is_none_or(|name| {
                        name != format!(
                            "{}-e{}.json",
                            record.project.as_str(),
                            record.epoch.value()
                        )
                        .as_str()
                    }) {
                        return Err(RouterLocalStorageError::InvalidRecordKey {
                            field: "snapshotInstallAttempt",
                        });
                    }
                    attempts.push((path, record));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if session.is_none() && progress.is_none() && attempts.is_empty() {
            return Ok(());
        }
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut imported = Vec::new();
        if let Some(state) = session {
            if read_runtime::<ClientSessionState>(&mut transaction, RuntimeStateKind::Session)
                .await?
                .is_none()
            {
                write_runtime(&mut transaction, RuntimeStateKind::Session, &state).await?;
            }
            imported.push(session_path);
        }
        if let Some(state) = progress {
            if read_runtime::<MachineNoticeProgress>(
                &mut transaction,
                RuntimeStateKind::NoticeProgress,
            )
            .await?
            .is_none()
            {
                write_runtime(&mut transaction, RuntimeStateKind::NoticeProgress, &state).await?;
            }
            imported.push(progress_path);
        }
        for (path, record) in attempts {
            insert_attempt(&mut transaction, &record).await?;
            imported.push(path);
        }
        transaction.commit().await?;
        for path in imported {
            remove_imported(&path).await?;
        }
        Ok(())
    }
}

async fn read_legacy<T: DeserializeOwned>(
    path: &Path,
    field: &'static str,
) -> Result<Option<T>, RouterLocalStorageError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > 1024 {
        return Err(RouterLocalStorageError::InvalidRecordKey { field });
    }
    let bytes = tokio::fs::read(path).await?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| RouterLocalStorageError::InvalidRecordKey { field })
}

async fn remove_imported(path: &PathBuf) -> Result<(), RouterLocalStorageError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    if let Some(parent) = path.parent() {
        tokio::fs::File::open(parent).await?.sync_all().await?;
    }
    Ok(())
}
