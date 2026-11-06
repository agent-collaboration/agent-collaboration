//! Debug-only causal barriers at the two D1 Router custody boundaries.
use std::{io, path::Path};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) async fn wait_at_socket(variable: &'static str, marker: &[u8]) -> io::Result<()> {
    let Some(path) = std::env::var_os(variable) else {
        return Ok(());
    };
    validate_barrier_socket(Path::new(&path))?;
    let mut barrier = tokio::net::UnixStream::connect(path).await?;
    barrier.write_all(marker).await?;
    let mut release = [0];
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        barrier.read_exact(&mut release),
    )
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

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
