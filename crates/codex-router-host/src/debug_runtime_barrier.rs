//! Socket authorization for the debug mail clock.
use std::{io, path::Path};
pub(crate) fn validate_barrier_socket(socket: &Path) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    if !socket.is_absolute() {
        return Err(io::Error::other(
            "debug runtime barrier socket must be absolute",
        ));
    }
    let parent = socket
        .parent()
        .ok_or_else(|| io::Error::other("debug barrier requires a dedicated directory"))?;
    let directory = std::fs::symlink_metadata(parent)?;
    let endpoint = std::fs::symlink_metadata(socket)?;
    let owner = rustix::process::geteuid().as_raw();
    if !directory.is_dir()
        || directory.uid() != owner
        || directory.permissions().mode() & 0o077 != 0
        || !endpoint.file_type().is_socket()
        || endpoint.uid() != owner
    {
        return Err(io::Error::other(
            "debug runtime barrier requires an owner-controlled socket in a private directory",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "debug_runtime_barrier_test.rs"]
mod tests;
