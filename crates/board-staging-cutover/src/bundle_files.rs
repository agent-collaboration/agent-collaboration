//! Private artifact files and the unchanged source directory's physical identity.
use crate::ProjectBundleError;
use message_board::CanonicalHash;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(crate) const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDirectoryFingerprint {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) entries: Vec<SourceEntryFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceEntryFingerprint {
    relative: PathBuf,
    device: u64,
    inode: u64,
    kind: SourceEntryKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum SourceEntryKind {
    Directory,
    File { bytes: u64, digest: CanonicalHash },
}

impl SourceDirectoryFingerprint {
    pub(crate) fn capture(directory: &Path) -> Result<Self, ProjectBundleError> {
        let metadata = owned_directory(directory, false)?;
        let mut entries = Vec::new();
        collect_source_entries(directory, directory, metadata.dev(), &mut entries)?;
        entries.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            entries,
        })
    }

    pub fn matches(&self, directory: &Path) -> Result<bool, ProjectBundleError> {
        Ok(self == &Self::capture(directory)?)
    }
}

fn collect_source_entries(
    root: &Path,
    directory: &Path,
    device: u64,
    entries: &mut Vec<SourceEntryFingerprint>,
) -> Result<(), ProjectBundleError> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.dev() != device {
            return Err(ProjectBundleError::UnsafePath);
        }
        if metadata.is_dir() {
            // Refuse mounts and symlinks rather than inventing a copy fallback.
            entries.push(SourceEntryFingerprint {
                relative: path
                    .strip_prefix(root)
                    .map_err(|_| ProjectBundleError::UnsafePath)?
                    .to_owned(),
                device: metadata.dev(),
                inode: metadata.ino(),
                kind: SourceEntryKind::Directory,
            });
            collect_source_entries(root, &path, device, entries)?;
        } else if metadata.is_file() {
            let mut file = open_regular(&path, false)?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher)?;
            let after = file.metadata()?;
            if metadata.dev() != after.dev()
                || metadata.ino() != after.ino()
                || metadata.len() != after.len()
                || metadata.mtime() != after.mtime()
                || metadata.mtime_nsec() != after.mtime_nsec()
                || metadata.ctime() != after.ctime()
                || metadata.ctime_nsec() != after.ctime_nsec()
            {
                return Err(ProjectBundleError::SourceChanged);
            }
            entries.push(SourceEntryFingerprint {
                relative: path
                    .strip_prefix(root)
                    .map_err(|_| ProjectBundleError::UnsafePath)?
                    .to_owned(),
                device: after.dev(),
                inode: after.ino(),
                kind: SourceEntryKind::File {
                    bytes: after.len(),
                    digest: hex_digest(hasher.finalize())?,
                },
            });
        } else {
            return Err(ProjectBundleError::UnsafePath);
        }
    }
    Ok(())
}

pub(crate) fn owned_directory(
    path: &Path,
    private: bool,
) -> Result<std::fs::Metadata, ProjectBundleError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || (private && metadata.mode() & 0o077 != 0)
    {
        return Err(ProjectBundleError::UnsafePath);
    }
    Ok(metadata)
}

pub(crate) fn open_regular(path: &Path, private: bool) -> Result<File, ProjectBundleError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || (private && metadata.mode() & 0o077 != 0)
    {
        return Err(ProjectBundleError::UnsafePath);
    }
    Ok(file)
}

pub(crate) fn write_frozen(path: &Path, bytes: &[u8]) -> Result<(), ProjectBundleError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.set_permissions(std::fs::Permissions::from_mode(0o400))?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn read_small<T: DeserializeOwned>(
    path: &Path,
) -> Result<(T, Vec<u8>), ProjectBundleError> {
    let file = open_regular(path, true)?;
    // Only the manifest/index uses this bound. Snapshot rows stay in separate pages.
    let mut bytes = Vec::new();
    file.take(MAX_INDEX_BYTES + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(|_| ProjectBundleError::InvalidArtifact)?
        > MAX_INDEX_BYTES
    {
        return Err(ProjectBundleError::InvalidArtifact);
    }
    Ok((serde_json::from_slice(&bytes)?, bytes))
}

pub(crate) fn bytes_digest(bytes: &[u8]) -> Result<CanonicalHash, ProjectBundleError> {
    hex_digest(Sha256::digest(bytes))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> Result<CanonicalHash, ProjectBundleError> {
    let hex = bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    hex.try_into()
        .map_err(|_| ProjectBundleError::InvalidArtifact)
}

pub(crate) fn commit_directory(
    parent: &Path,
    partial: &Path,
    final_name: &str,
) -> Result<PathBuf, ProjectBundleError> {
    let destination = parent.join(final_name);
    move_directory_no_replace(partial, &destination)?;
    Ok(destination)
}

pub(crate) fn move_directory_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
    let source_parent = source.parent().ok_or_else(invalid)?;
    let destination_parent = destination.parent().ok_or_else(invalid)?;
    let source_file = File::open(source_parent)?;
    let destination_file = File::open(destination_parent)?;
    File::open(source)?.sync_all()?;
    rustix::fs::renameat_with(
        &source_file,
        source.file_name().ok_or_else(invalid)?,
        &destination_file,
        destination.file_name().ok_or_else(invalid)?,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    source_file.sync_all()?;
    if source_parent != destination_parent {
        destination_file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "bundle_files_test.rs"]
mod tests;

pub(crate) fn backup_directory(
    router_root: &Path,
    run_id: uuid::Uuid,
    project: &message_board::ProjectId,
) -> Result<PathBuf, ProjectBundleError> {
    let timestamp = run_id
        .get_timestamp()
        .ok_or(ProjectBundleError::InvalidPreparation)?
        .to_unix();
    let date = chrono::DateTime::from_timestamp(
        i64::try_from(timestamp.0).map_err(|_| ProjectBundleError::InvalidPreparation)?,
        timestamp.1,
    )
    .ok_or(ProjectBundleError::InvalidPreparation)?
    .format("%Y-%m-%d")
    .to_string();
    Ok(router_root
        .join("staging-cutover-backups")
        .join(format!("{date}-{run_id}"))
        .join(project.as_str()))
}
