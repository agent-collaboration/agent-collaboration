//! Listener and shutdown ownership for the modern workspace host.
use crate::{
    OrganizationSignInLink, OrganizationSignInLinkError, TokioWorkspaceHost, WorkspaceLink,
};
use central_storage::StoreFailure;
use collaboration_central_core::SignInTrustSettings;
use router_clock::RuntimeClock;
use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum TokioWorkspaceServerError {
    #[error("workspace host storage or sign-in configuration is unavailable")]
    Host(#[from] StoreFailure),
    #[error("public workspace listener failed")]
    Listener(#[from] io::Error),
    #[error(transparent)]
    SignIn(#[from] OrganizationSignInLinkError),
    #[error("public workspace listener task failed")]
    Task(#[from] tokio::task::JoinError),
}

pub struct TokioWorkspaceServer {
    address: SocketAddr,
    sign_in_socket: PathBuf,
    host: Arc<TokioWorkspaceHost>,
    link: WorkspaceLink,
    sign_in: Option<OrganizationSignInLink>,
    stop: CancellationToken,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl TokioWorkspaceServer {
    /// The caller holds the lock on its private root through startup and shutdown.
    pub async fn start(
        root: &Path,
        listen: SocketAddr,
        sign_in: SignInTrustSettings,
        clock: Arc<dyn RuntimeClock>,
        owner_uid: u32,
    ) -> Result<Self, TokioWorkspaceServerError> {
        let sign_in_socket = root.join("signin.sock");
        OrganizationSignInLink::clear_stale_owned_socket(&sign_in_socket, owner_uid)
            .map_err(OrganizationSignInLinkError::from)?;
        let listener = tokio::net::TcpListener::bind(listen).await?;
        let address = listener.local_addr()?;
        let host = Arc::new(TokioWorkspaceHost::open_with_signin(root, clock, sign_in).await?);
        let sign_in = match OrganizationSignInLink::start(host.clone(), &sign_in_socket) {
            Ok(link) => link,
            Err(error) => {
                host.close().await;
                return Err(error.into());
            }
        };
        let link = WorkspaceLink::new(host.clone());
        let router = link.router();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            address,
            sign_in_socket,
            host,
            link,
            sign_in: Some(sign_in),
            stop,
            task: Some(task),
        })
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    #[must_use]
    pub fn sign_in_socket(&self) -> &Path {
        &self.sign_in_socket
    }

    pub async fn shutdown(mut self) -> Result<(), TokioWorkspaceServerError> {
        self.link.begin_shutdown();
        self.stop.cancel();
        let public = self.task.take();
        let private = self.sign_in.take();
        let (public, private) = tokio::join!(
            async {
                if let Some(task) = public {
                    task.await??;
                }
                Ok::<_, TokioWorkspaceServerError>(())
            },
            async {
                if let Some(sign_in) = private {
                    sign_in.shutdown().await?;
                }
                Ok::<_, TokioWorkspaceServerError>(())
            }
        );
        self.link.shutdown().await;
        self.host.close().await;
        public?;
        private
    }
}

impl Drop for TokioWorkspaceServer {
    fn drop(&mut self) {
        self.link.begin_shutdown();
        self.stop.cancel();
    }
}
