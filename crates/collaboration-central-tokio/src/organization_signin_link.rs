//! A dedicated owner-only Unix listener for the modern private sign-in routes.

use crate::TokioWorkspaceHost;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum OrganizationSignInLinkError {
    #[error("private sign-in listener failed")]
    Listener(#[from] std::io::Error),
    #[error("private sign-in listener task failed")]
    Task(#[from] tokio::task::JoinError),
}

/// The caller shuts down the link before closing its shared workspace host.
pub struct OrganizationSignInLink {
    socket: PathBuf,
    socket_device: u64,
    socket_inode: u64,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<Result<(), std::io::Error>>>,
}

impl OrganizationSignInLink {
    /// Binding is exclusive: an existing path is never removed or reused.
    pub fn start(
        host: Arc<TokioWorkspaceHost>,
        socket: &Path,
    ) -> Result<Self, OrganizationSignInLinkError> {
        let listener = tokio::net::UnixListener::bind(socket)?;
        let metadata = std::fs::symlink_metadata(socket)?;
        if let Err(error) = std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        {
            remove_owned_socket(socket, metadata.dev(), metadata.ino())?;
            return Err(error.into());
        }
        let router = collaboration_central_core::organization_signin_router(host);
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            socket: socket.to_owned(),
            socket_device: metadata.dev(),
            socket_inode: metadata.ino(),
            stop,
            task: Some(task),
        })
    }

    pub async fn shutdown(mut self) -> Result<(), OrganizationSignInLinkError> {
        self.stop.cancel();
        let served = match self.task.take() {
            Some(task) => task.await,
            None => Ok(Ok(())),
        };
        // Do not remove a replacement created by someone else at the same path.
        let cleanup = remove_owned_socket(&self.socket, self.socket_device, self.socket_inode);
        served??;
        cleanup?;
        Ok(())
    }
}

fn remove_owned_socket(socket: &Path, device: u64, inode: u64) -> Result<(), std::io::Error> {
    match std::fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.dev() == device && metadata.ino() == inode => {
            std::fs::remove_file(socket)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl Drop for OrganizationSignInLink {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
