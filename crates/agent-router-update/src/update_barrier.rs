//! Debug-build causal barriers inside each update step, so a test can hold the real
//! process at a step boundary and kill it there. Release builds compile none of this.
use std::io;

/// Names the boundary a debug update process has reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpdateBarrier {
    /// Step 0001 committed the ledger migration but hasn't recorded itself done.
    LedgerMigrated,
    /// Step 0001 recorded itself done.
    LedgerStepDone,
}

impl UpdateBarrier {
    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "debug barriers only")
    )]
    const fn marker(self) -> &'static [u8] {
        match self {
            Self::LedgerMigrated => b"step-0001:migrated\n",
            Self::LedgerStepDone => b"step-0001:done\n",
        }
    }
}

#[cfg(debug_assertions)]
const BARRIER_SOCKET_VARIABLE: &str = "AGENT_ROUTER_UPDATE_DEBUG_BARRIER_SOCKET";

/// Waits at `barrier` when a debug test named a barrier socket; otherwise returns at once.
pub(crate) async fn wait_at(barrier: UpdateBarrier) -> io::Result<()> {
    #[cfg(debug_assertions)]
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let Some(path) = std::env::var_os(BARRIER_SOCKET_VARIABLE) else {
            return Ok(());
        };
        let path = std::path::PathBuf::from(path);
        validate_barrier_socket(&path)?;
        let mut socket = tokio::net::UnixStream::connect(&path).await?;
        socket.write_all(barrier.marker()).await?;
        let mut release = [0];
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            socket.read_exact(&mut release),
        )
        .await
        .map_err(io::Error::other)??;
    }
    #[cfg(not(debug_assertions))]
    let _ = barrier;
    Ok(())
}

#[cfg(debug_assertions)]
fn validate_barrier_socket(socket: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let owner = nix::unistd::geteuid().as_raw();
    let parent = socket
        .parent()
        .filter(|_| socket.is_absolute())
        .ok_or_else(|| io::Error::other("update barrier socket must be an absolute path"))?;
    let directory = std::fs::symlink_metadata(parent)?;
    let endpoint = std::fs::symlink_metadata(socket)?;
    if !directory.is_dir()
        || directory.uid() != owner
        || directory.permissions().mode() & 0o077 != 0
        || !endpoint.file_type().is_socket()
        || endpoint.uid() != owner
    {
        return Err(io::Error::other(
            "update barrier requires an owner-controlled socket in a private directory",
        ));
    }
    Ok(())
}
