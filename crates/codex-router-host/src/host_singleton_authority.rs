//! Exclusive host singleton authority and owner-only operator socket binding.

use std::ffi::OsStr;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::Permissions;
use std::fs::TryLockError;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;
use tokio::net::UnixListener;

use crate::HostCoordinationPaths;

/// Holds Host singleton authority without removing or binding its operator socket.
pub struct HostInstanceLock {
    lock_file: File,
}

impl HostInstance {
    /// Clears refused product endpoints while this instance holds singleton authority.
    /// Binders retain their never-unlink contract; live or uncertain endpoints survive.
    pub async fn clear_stale_endpoint_sockets(&self, directory: &Path) -> std::io::Result<()> {
        validate_private_runtime_directory(directory)?;
        for name in ["control.sock", "codex-native.sock", "codex-acp.sock"] {
            clear_refused_socket(&directory.join(name)).await?;
        }
        let provider_directory = directory.join("router-sessions");
        match std::fs::symlink_metadata(&provider_directory) {
            Ok(_) => validate_private_runtime_directory(&provider_directory)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
        for name in ["claude-local.sock", "cursor-local.sock"] {
            clear_refused_socket(&provider_directory.join(name)).await?;
        }
        Ok(())
    }
}

impl HostInstanceLock {
    /// Acquires the same nonblocking lock used by the running Host.
    pub fn acquire(paths: &HostCoordinationPaths) -> Result<Self, InstanceAcquireError> {
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(paths.instance_lock())
            .map_err(InstanceAcquireError::OpenLock)?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(InstanceAcquireError::AlreadyRunning),
            Err(TryLockError::Error(error)) => {
                return Err(InstanceAcquireError::AcquireLock(error));
            }
        }
        lock_file
            .set_permissions(Permissions::from_mode(0o600))
            .map_err(InstanceAcquireError::SetLockPermissions)?;

        Ok(Self { lock_file })
    }
}

/// Exclusive host instance and its private operator listener.
pub struct HostInstance {
    listener: UnixListener,
    socket_path: PathBuf,
    lock_file: File,
}

impl HostInstance {
    /// Acquires singleton authority before removing or binding the socket.
    pub fn acquire(paths: HostCoordinationPaths) -> Result<Self, InstanceAcquireError> {
        let authority = HostInstanceLock::acquire(&paths)?;
        Self::bind_with_lock(paths, authority.lock_file)
    }

    /// Consumes the continuously held descriptor inherited through stdin.
    pub fn acquire_inherited(
        paths: HostCoordinationPaths,
        marker: &OsStr,
    ) -> Result<Self, InstanceAcquireError> {
        if marker != OsStr::new(inherited_lock_marker()) {
            return Err(InstanceAcquireError::InheritedMarkerMismatch);
        }
        let inherited = rustix::stdio::stdin();
        let inherited_stat =
            rustix::fs::fstat(inherited).map_err(InstanceAcquireError::InspectInheritedLock)?;
        let artifact_stat = rustix::fs::stat(paths.instance_lock())
            .map_err(InstanceAcquireError::InspectLockArtifact)?;
        if inherited_stat.st_dev != artifact_stat.st_dev
            || inherited_stat.st_ino != artifact_stat.st_ino
        {
            return Err(InstanceAcquireError::InheritedLockMismatch);
        }

        let authority =
            rustix::io::dup(inherited).map_err(InstanceAcquireError::DuplicateInheritedLock)?;
        rustix::io::fcntl_setfd(&authority, rustix::io::FdFlags::CLOEXEC)
            .map_err(InstanceAcquireError::RestoreCloseOnExec)?;
        let null_input =
            File::open("/dev/null").map_err(InstanceAcquireError::RestoreStandardInput)?;
        rustix::stdio::dup2_stdin(&null_input)
            .map_err(|error| InstanceAcquireError::RestoreStandardInput(error.into()))?;
        let lock_file = File::from(authority);

        Self::bind_with_lock(paths, lock_file)
    }

    /// Places singleton authority on stdin for one whole-Host replacement exec.
    pub fn prepare_lock_for_exec(&self) -> Result<(), InstanceAcquireError> {
        rustix::stdio::dup2_stdin(&self.lock_file)
            .map_err(InstanceAcquireError::PrepareInheritedLock)?;
        rustix::io::fcntl_setfd(rustix::stdio::stdin(), rustix::io::FdFlags::empty())
            .map_err(InstanceAcquireError::PrepareInheritedLock)
    }

    /// Replaces the prepared stdin duplicate when foreground exec returns.
    pub fn release_prepared_lock_after_exec_failure(&self) -> Result<(), InstanceAcquireError> {
        let null_input = File::open("/dev/null")
            .map_err(InstanceAcquireError::ReleasePreparedLockAfterExecFailure)?;
        rustix::stdio::dup2_stdin(&null_input).map_err(|error| {
            InstanceAcquireError::ReleasePreparedLockAfterExecFailure(error.into())
        })
    }

    /// Removes the published pathname immediately before same-process exec.
    pub fn remove_operator_socket_for_exec(&self) -> Result<(), InstanceAcquireError> {
        match std::fs::remove_file(&self.socket_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(InstanceAcquireError::RemoveOperatorSocketForExec(error)),
        }
    }

    fn bind_with_lock(
        paths: HostCoordinationPaths,
        lock_file: File,
    ) -> Result<Self, InstanceAcquireError> {
        match std::fs::remove_file(paths.operator_socket()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(InstanceAcquireError::RemoveStaleSocket(error)),
        }
        let listener = UnixListener::bind(paths.operator_socket())
            .map_err(InstanceAcquireError::BindOperatorSocket)?;
        std::fs::set_permissions(paths.operator_socket(), Permissions::from_mode(0o600))
            .map_err(InstanceAcquireError::SetSocketPermissions)?;

        Ok(Self {
            listener,
            socket_path: paths.operator_socket().to_owned(),
            lock_file,
        })
    }

    /// Returns the owner-only Tokio listener.
    #[must_use]
    pub const fn listener(&self) -> &UnixListener {
        &self.listener
    }
}

fn validate_private_runtime_directory(directory: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(std::io::Error::other(
            "endpoint runtime directory must be owner-private",
        ));
    }
    Ok(())
}

async fn clear_refused_socket(socket_path: &Path) -> std::io::Result<()> {
    let before = match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !before.file_type().is_socket() || before.uid() != rustix::process::getuid().as_raw() {
        return Err(std::io::Error::other(
            "endpoint path is not an owned socket",
        ));
    }
    // Only ECONNREFUSED permits removal. A full backlog or failed probe is uncertain.
    match tokio::time::timeout(
        Duration::from_millis(250),
        tokio::net::UnixStream::connect(socket_path),
    )
    .await
    {
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
        Ok(Ok(_stream)) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "endpoint socket is live",
            ));
        }
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, error)),
    }
    let after = std::fs::symlink_metadata(socket_path)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(std::io::Error::other(
            "endpoint socket changed during probe",
        ));
    }
    std::fs::remove_file(socket_path)
}

/// Stable private handoff protocol passed only by whole-Host replacement.
#[must_use]
pub const fn inherited_lock_marker() -> &'static str {
    "codex-router-host-handoff/v1"
}

/// Private environment key carrying the inherited-lock handoff marker.
#[must_use]
pub const fn inherited_lock_environment() -> &'static str {
    "CODEX_ROUTER_HOST_INHERITED_LOCK"
}

impl Drop for HostInstance {
    fn drop(&mut self) {
        let _remove_result = std::fs::remove_file(&self.socket_path);
    }
}

/// Singleton authority or socket-publication failure.
#[derive(Debug, Error)]
pub enum InstanceAcquireError {
    /// Another process currently owns the stable lock artifact.
    #[error("shared Codex host is already running")]
    AlreadyRunning,
    /// Stable lock artifact could not be opened.
    #[error("failed opening host instance lock: {0}")]
    OpenLock(#[source] std::io::Error),
    /// Exclusive nonblocking lock operation failed.
    #[error("failed acquiring host instance lock: {0}")]
    AcquireLock(#[source] std::io::Error),
    /// Owner-private lock permissions could not be enforced.
    #[error("failed setting host instance-lock permissions: {0}")]
    SetLockPermissions(#[source] std::io::Error),
    /// The lock owner could not remove a stale socket pathname.
    #[error("failed removing stale operator socket: {0}")]
    RemoveStaleSocket(#[source] std::io::Error),
    /// The lock owner could not bind the operator socket.
    #[error("failed binding operator socket: {0}")]
    BindOperatorSocket(#[source] std::io::Error),
    /// Owner-only socket permissions could not be enforced.
    #[error("failed setting operator-socket permissions: {0}")]
    SetSocketPermissions(#[source] std::io::Error),
    /// Replacement bootstrap did not carry the supported handoff marker.
    #[error("inherited host lock marker does not match this binary")]
    InheritedMarkerMismatch,
    /// Inherited stdin metadata could not be inspected.
    #[error("failed inspecting inherited host lock: {0}")]
    InspectInheritedLock(#[source] rustix::io::Errno),
    /// Configured stable artifact metadata could not be inspected.
    #[error("failed inspecting configured host lock artifact: {0}")]
    InspectLockArtifact(#[source] rustix::io::Errno),
    /// Inherited stdin did not refer to the configured stable artifact.
    #[error("inherited host lock does not match the configured artifact")]
    InheritedLockMismatch,
    /// Continuously held inherited descriptor could not be duplicated.
    #[error("failed retaining inherited host lock: {0}")]
    DuplicateInheritedLock(#[source] rustix::io::Errno),
    /// Close-on-exec could not be restored before child spawning.
    #[error("failed restoring close-on-exec for host authority: {0}")]
    RestoreCloseOnExec(#[source] rustix::io::Errno),
    /// Replacement bootstrap could not restore a safe standard-input descriptor.
    #[error("failed restoring standard input after retaining inherited host authority: {0}")]
    RestoreStandardInput(#[source] std::io::Error),
    /// Singleton authority could not be placed on stdin for re-exec.
    #[error("failed preparing inherited host lock: {0}")]
    PrepareInheritedLock(#[source] rustix::io::Errno),
    /// A failed exec left singleton authority duplicated on stdin.
    #[error("failed releasing prepared host lock after exec failure: {0}")]
    ReleasePreparedLockAfterExecFailure(#[source] std::io::Error),
    /// Host replacement could not unpublish the old operator socket.
    #[error("failed removing operator socket before host replacement: {0}")]
    RemoveOperatorSocketForExec(#[source] std::io::Error),
}
