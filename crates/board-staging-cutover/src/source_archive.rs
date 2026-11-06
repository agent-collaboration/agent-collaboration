//! Archive the entire former writer directory only after Central publication.
use crate::bundle_files::{backup_directory, owned_directory};
use crate::{OfflineHost, PreparedProjectBundle, ProjectBundleError, PublishedProject};
use serde::Serialize;
use std::{
    fs::{DirBuilder, File},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum SourceArchiveError {
    #[error("publication confirmation belongs to another prepared bundle")]
    ConfirmationMismatch,
    #[error("archive source or destination differs from the declared offline Host")]
    HostMismatch,
    #[error("the source directory changed after immutable preparation")]
    SourceChanged,
    #[error("source and backup are both present; neither will be overwritten")]
    ConflictingPaths,
    #[error("neither the original directory nor its verified backup is present")]
    SourceMissing,
    #[error("the existing backup does not match the original directory")]
    BackupMismatch,
    #[error("the source and backup must be on the same filesystem")]
    CrossDevice,
    #[error("the prepared archive data is invalid")]
    Bundle(#[from] ProjectBundleError),
    #[error("cannot archive the original directory")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SourceArchiveOutcome {
    Archived { directory: PathBuf },
    PreviouslyArchived { directory: PathBuf },
}

enum DirectoryPresence {
    Absent,
    Present(std::fs::Metadata),
}

pub fn archive_confirmed_source(
    host: &mut OfflineHost,
    bundle: &PreparedProjectBundle,
    confirmed: PublishedProject,
) -> Result<SourceArchiveOutcome, SourceArchiveError> {
    // An artifact change after publication cannot redirect the source or move.
    let checked = PreparedProjectBundle::open(bundle.directory())?;
    if !confirmed.matches_bundle(bundle)? || !confirmed.matches_bundle(&checked)? {
        return Err(SourceArchiveError::ConfirmationMismatch);
    }
    let manifest = checked.manifest();
    let selected = &manifest.source.selected;
    let source = host
        .collaboration_directory()
        .join("projects")
        .join(selected.project.as_str());
    let backup = backup_directory(host.router_root(), manifest.run_id, &selected.project)?;
    if selected.peer != host.peer()
        || source != manifest.source_directory
        || backup != manifest.backup_directory
    {
        return Err(SourceArchiveError::HostMismatch);
    }
    let source_parent = source.parent().ok_or(SourceArchiveError::HostMismatch)?;
    owned_directory(source_parent, false)?;
    let backup_root = host.router_root().join("staging-cutover-backups");
    ensure_private_directory(&backup_root)?;
    let backup_parent = backup.parent().ok_or(SourceArchiveError::HostMismatch)?;
    ensure_private_directory(backup_parent)?;
    let backup_parent_file = File::open(backup_parent)?;
    let source_presence = directory_presence(&source)?;
    let backup_presence = directory_presence(&backup)?;
    match (source_presence, backup_presence) {
        (DirectoryPresence::Present(_), DirectoryPresence::Present(_)) => {
            return Err(SourceArchiveError::ConflictingPaths);
        }
        (DirectoryPresence::Absent, DirectoryPresence::Absent) => {
            return Err(SourceArchiveError::SourceMissing);
        }
        (DirectoryPresence::Absent, DirectoryPresence::Present(_)) => {
            if !manifest.source_fingerprint.matches(&backup)? {
                return Err(SourceArchiveError::BackupMismatch);
            }
            return Ok(SourceArchiveOutcome::PreviouslyArchived { directory: backup });
        }
        (DirectoryPresence::Present(metadata), DirectoryPresence::Absent) => {
            if metadata.dev() != backup_parent_file.metadata()?.dev() {
                return Err(SourceArchiveError::CrossDevice);
            }
            if !manifest.source_fingerprint.matches(&source)? {
                return Err(SourceArchiveError::SourceChanged);
            }
        }
    }
    // Native handles closed before fingerprinting. The shared no-overwrite
    // primitive moves all eN/WAL/SHM siblings and syncs both parents.
    crate::bundle_files::move_directory_no_replace(&source, &backup)?;
    Ok(SourceArchiveOutcome::Archived { directory: backup })
}

fn directory_presence(path: &Path) -> Result<DirectoryPresence, SourceArchiveError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(DirectoryPresence::Present(owned_directory(path, false)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DirectoryPresence::Absent),
        Err(error) => Err(error.into()),
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), SourceArchiveError> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {
            let parent = path.parent().ok_or(SourceArchiveError::HostMismatch)?;
            File::open(parent)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    owned_directory(path, true)?;
    Ok(())
}
