//! Commit validated legacy state before deleting any source file; never overwrite a committed row.
use super::{
    ClientSessionState, DeserializeOwned, FleetCache, MachineNoticeProgress,
    RouterLocalStorageError, SnapshotInstallAttempt, insert_attempt,
    sessions::{insert_session, read_session},
};
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "fleet_runtime_import_test.rs"]
mod tests;

enum LegacyNoticeProgress {
    Missing,
    Bound(fleet_protocol::NoticePosition),
    Orphan {
        handled_through: Option<fleet_protocol::NoticePosition>,
    },
}

// An orphan is never imported. Inspect only this optional numeric field for
// the warning; unknown payload fields and raw bytes must not enter the log.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadableOrphanProgress {
    handled_through: Option<fleet_protocol::NoticePosition>,
}

async fn read_notice_progress(
    path: &Path,
    session: Option<&ClientSessionState>,
) -> Result<LegacyNoticeProgress, RouterLocalStorageError> {
    if session.is_some() {
        return Ok(
            match read_legacy::<MachineNoticeProgress>(path, "machineNoticeProgress").await? {
                Some(progress) => LegacyNoticeProgress::Bound(progress.handled_through),
                None => LegacyNoticeProgress::Missing,
            },
        );
    }
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LegacyNoticeProgress::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(RouterLocalStorageError::InvalidRecordKey {
            field: "machineNoticeProgress",
        });
    }
    let handled_through = read_legacy::<ReadableOrphanProgress>(path, "machineNoticeProgress")
        .await
        .ok()
        .flatten()
        .and_then(|progress| progress.handled_through);
    Ok(LegacyNoticeProgress::Orphan { handled_through })
}

impl FleetCache {
    pub async fn import_legacy_runtime(
        &self,
        directory: &Path,
    ) -> Result<(), RouterLocalStorageError> {
        for path in self.commit_legacy_runtime(directory).await? {
            remove_imported(&path).await?;
        }
        Ok(())
    }

    async fn commit_legacy_runtime(
        &self,
        directory: &Path,
    ) -> Result<Vec<PathBuf>, RouterLocalStorageError> {
        let session_path = directory.join("machine-session/state.json");
        let progress_path = directory.join("machine-session/machine-notice-progress.json");
        let session: Option<ClientSessionState> =
            read_legacy(&session_path, "machineSessionState").await?;
        let progress = read_notice_progress(&progress_path, session.as_ref()).await?;
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
        if session.is_none()
            && matches!(&progress, LegacyNoticeProgress::Missing)
            && attempts.is_empty()
        {
            return Ok(Vec::new());
        }
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut imported = Vec::new();
        let existing = if session.is_some() {
            read_session(&mut transaction).await?
        } else {
            None
        };
        if let Some(state) = session {
            if existing.is_none() {
                insert_session(
                    &mut transaction,
                    &state,
                    match &progress {
                        LegacyNoticeProgress::Bound(position) => *position,
                        LegacyNoticeProgress::Missing | LegacyNoticeProgress::Orphan { .. } => {
                            fleet_protocol::NoticePosition::new(0)
                        }
                    },
                )
                .await?;
            }
            imported.push(session_path);
        }
        if !matches!(&progress, LegacyNoticeProgress::Missing) {
            imported.push(progress_path);
        }
        for (path, record) in attempts {
            insert_attempt(&mut transaction, &record).await?;
            imported.push(path);
        }
        transaction.commit().await?;
        match progress {
            LegacyNoticeProgress::Orphan {
                handled_through: Some(position),
            } => tracing::warn!(
                event = "fleet.runtime.orphanNoticeProgressDiscarded",
                handled_through = position.value(),
                "discarding orphan notice progress without its native session; notices will be re-handled"
            ),
            LegacyNoticeProgress::Orphan {
                handled_through: None,
            } => tracing::warn!(
                event = "fleet.runtime.orphanNoticeProgressDiscarded",
                handled_through = "unavailable",
                "discarding orphan notice progress without its native session; notices will be re-handled"
            ),
            LegacyNoticeProgress::Missing | LegacyNoticeProgress::Bound(_) => {}
        }
        Ok(imported)
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
