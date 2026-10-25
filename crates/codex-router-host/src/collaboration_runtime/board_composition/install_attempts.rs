//! Root retains one native install attempt for each granted project epoch.
use fleet_sync_protocol::{Epoch, InstallAttemptId};
use message_board::ProjectId;
use runtime_primitives::{IdSource, UuidV7Ids};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallAttemptRecord {
    project: ProjectId,
    epoch: Epoch,
    attempt: InstallAttemptId,
}

/// The Host calls this while holding the existing per-project owner.
pub(super) async fn recorded_attempt(
    router_directory: &Path,
    project: &ProjectId,
    epoch: Epoch,
) -> io::Result<InstallAttemptId> {
    let router_directory = router_directory.to_owned();
    let project = project.clone();
    tokio::task::spawn_blocking(move || load_or_record(&router_directory, &project, epoch))
        .await
        .map_err(io::Error::other)?
}

fn record_path(directory: &Path, project: &ProjectId, epoch: Epoch) -> PathBuf {
    directory.join(format!("{}-e{}.json", project.as_str(), epoch.value()))
}

fn load_or_record(
    router_directory: &Path,
    project: &ProjectId,
    epoch: Epoch,
) -> io::Result<InstallAttemptId> {
    let directory = router_directory.join("snapshot-install-attempts");
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !std::fs::symlink_metadata(&directory)?.is_dir() {
                return Err(io::Error::other("install attempt home is not a directory"));
            }
        }
        Err(error) => return Err(error),
    }
    let path = record_path(&directory, project, epoch);
    match read_record(&path, project, epoch) {
        Ok(record) => {
            sync_record(&path, &directory, router_directory)?;
            return Ok(record.attempt);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let record = InstallAttemptRecord {
        project: project.clone(),
        epoch,
        attempt: InstallAttemptId::new(UuidV7Ids.new_id()).map_err(io::Error::other)?,
    };
    let temporary = directory.join(format!(".install-attempt-{}.tmp", record.attempt));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(&record).map_err(io::Error::other)?)?;
        file.sync_all()?;
        // A cancelled async caller can leave its blocking write in flight.
        // Publish without replacing another creator's complete choice.
        match std::fs::hard_link(&temporary, &path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let selected = read_record(&path, project, epoch)?;
        sync_record(&path, &directory, router_directory)?;
        Ok(selected.attempt)
    })();
    let cleanup = std::fs::remove_file(&temporary);
    let attempt = result?;
    cleanup?;
    Ok(attempt)
}

fn read_record(path: &Path, project: &ProjectId, epoch: Epoch) -> io::Result<InstallAttemptRecord> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > 1024 {
        return Err(io::Error::other("invalid install attempt record file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        return Err(io::Error::other("install attempt record is too large"));
    }
    // Native Epoch and InstallAttemptId deserializers call their validated
    // constructors; a corrupt identity never causes a replacement attempt.
    let record: InstallAttemptRecord = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if &record.project != project || record.epoch != epoch {
        return Err(io::Error::other(
            "install attempt record names another grant",
        ));
    }
    Ok(record)
}

fn sync_record(path: &Path, directory: &Path, router_directory: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()?;
    File::open(directory)?.sync_all()?;
    File::open(router_directory)?.sync_all()
}

#[cfg(test)]
#[path = "tests/install_attempts.rs"]
mod tests;
