//! A Router session's scratch folder, `<router root>/scratch/<scope>`, and the
//! files that coordinate its use across programs (R17).
//!
//! Scratch folders are created by whichever program creates the session (the
//! Host or another client), so the Host's memory alone cannot coordinate
//! creating and removing them. Each scope has a lock file,
//! `scratch/.locks/<scope>.lock`: creating or opening a scope holds it shared
//! for the moment of creation and the use stamp; the Host's sweep holds it
//! exclusive, without waiting, while it rechecks and renames a folder away.
//! Use is recorded in the folder itself by touching `.last-used`, so the
//! folder's newest change covers use as well as writes and survives restarts.

use std::{
    fs::{DirBuilder, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Folder under the Router root that holds every scratch scope.
pub const SCRATCH_DIRECTORY_NAME: &str = "scratch";
/// Marker touched whenever a scope is created, opened or closed.
pub const SCRATCH_LAST_USED_FILE_NAME: &str = ".last-used";
/// Folder under the scratch root that holds one lock file per scope.
const SCRATCH_LOCKS_DIRECTORY_NAME: &str = ".locks";

/// `<router root>/scratch`.
#[must_use]
pub fn router_scratch_root(router_root: &Path) -> PathBuf {
    router_root.join(SCRATCH_DIRECTORY_NAME)
}

/// Creates the scope folder if it is missing (owner-private) and records a use,
/// holding the scope's lock shared so a sweep can never remove it midway. A
/// sweep that already renamed the folder away leaves a fresh one to be created.
pub fn prepare_scratch_scope(scope_folder: &Path) -> io::Result<()> {
    let (scratch_root, scope) = scope_parts(scope_folder)?;
    let lock = open_scope_lock(scratch_root, scope)?;
    lock.lock_shared()?;
    create_private_folder(scope_folder)?;
    record_scope_use(scope_folder)
}

/// The exclusive hold a sweep takes on one scope; released on drop.
#[derive(Debug)]
pub struct ScratchScopeExclusiveLock {
    _lock: File,
}

/// Takes the scope's lock exclusively without waiting. `Ok(None)` means a
/// creator or opener holds it now, so the sweep skips this scope.
pub fn try_lock_scratch_scope_exclusively(
    scratch_root: &Path,
    scope: &str,
) -> io::Result<Option<ScratchScopeExclusiveLock>> {
    let lock = open_scope_lock(scratch_root, scope)?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(ScratchScopeExclusiveLock { _lock: lock })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

/// The newest modification time of the folder and everything inside it,
/// without following symbolic links. Any entry that cannot be read makes the
/// whole answer an error, so a folder whose use is unknown is never removed.
pub fn scratch_scope_last_change(scope_folder: &Path) -> io::Result<SystemTime> {
    let metadata = std::fs::symlink_metadata(scope_folder)?;
    let mut newest = metadata.modified()?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(scope_folder)? {
            let changed = scratch_scope_last_change(&entry?.path())?;
            newest = newest.max(changed);
        }
    }
    Ok(newest)
}

fn record_scope_use(scope_folder: &Path) -> io::Result<()> {
    let marker = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(scope_folder.join(SCRATCH_LAST_USED_FILE_NAME))?;
    marker.set_modified(SystemTime::now())
}

fn open_scope_lock(scratch_root: &Path, scope: &str) -> io::Result<File> {
    let locks = scratch_root.join(SCRATCH_LOCKS_DIRECTORY_NAME);
    create_private_folder(&locks)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(locks.join(format!("{scope}.lock")))
}

fn create_private_folder(folder: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
    let metadata = std::fs::symlink_metadata(folder)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other("scratch folder is not owner-private"));
    }
    Ok(())
}

fn scope_parts(scope_folder: &Path) -> io::Result<(&Path, &str)> {
    let scope = scope_folder
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.starts_with('.'))
        .ok_or_else(|| io::Error::other("scratch scope folder has no usable name"))?;
    let scratch_root = scope_folder
        .parent()
        .ok_or_else(|| io::Error::other("scratch scope folder has no scratch root"))?;
    Ok((scratch_root, scope))
}

#[cfg(test)]
#[path = "router_scratch_scopes_test.rs"]
mod tests;
