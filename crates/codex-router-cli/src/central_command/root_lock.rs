//! Exclusive ownership of the configured central state directory.
use super::CentralCommandError;
use std::{
    fs::OpenOptions,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

pub(super) fn lock_root(root: &Path) -> Result<std::fs::File, CentralCommandError> {
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput(
            "central root must be absolute",
        ));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(CentralCommandError::InvalidInput(
            "central root must be a private directory owned by this account",
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("central.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| CentralCommandError::InvalidInput("central root is already in use"))?;
    Ok(file)
}
