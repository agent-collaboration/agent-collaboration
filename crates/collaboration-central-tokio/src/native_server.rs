//! Native listener and shutdown ownership for the core's HTTP application.

use crate::{TokioCentralPlatform, TokioPlatformError};
use collaboration_central_core::{
    AccessKeyCache, AccessPolicy, CentralApplication, central_router,
};
use fleet_sync_lease::RegistryConfig;
use runtime_primitives::SystemClock;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub struct TokioSettings {
    pub root: PathBuf,
    pub legacy_organization: fleet_identity_protocol::OrganizationId,
    pub access_policy: AccessPolicy,
    pub jwks_url: String,
    pub registry: RegistryConfig,
    pub delivery: crate::DeliveryConfig,
    pub holder_answer_timeout: std::time::Duration,
    pub copy_ack_wait: std::time::Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum TokioServerError {
    #[error(transparent)]
    Platform(#[from] TokioPlatformError),
    #[error("invalid Access policy")]
    AccessPolicy(#[from] collaboration_central_core::AccessRefusal),
    #[error("native HTTP listener failed")]
    Listener(#[from] std::io::Error),
    #[error("native HTTP task failed")]
    Task(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Store(#[from] durable_steps::StoreError),
}

pub struct TokioServer {
    base_url: String,
    platform: TokioCentralPlatform,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<Result<(), std::io::Error>>>,
    /// The private sign-in link, served only on its Unix socket.
    link: Option<tokio::task::JoinHandle<Result<(), std::io::Error>>>,
    /// Removed after the link stops; another process may own an existing one.
    link_socket: Option<std::path::PathBuf>,
}

impl TokioServer {
    pub async fn start(settings: TokioSettings) -> Result<Self, TokioServerError> {
        Self::start_on(settings, SocketAddr::from(([127, 0, 0, 1], 0))).await
    }

    pub async fn start_on(
        settings: TokioSettings,
        listen: SocketAddr,
    ) -> Result<Self, TokioServerError> {
        let platform = TokioCentralPlatform::new(
            &settings.jwks_url,
            Arc::new(SystemClock),
            crate::native_logging::observe,
            &settings.root,
            settings.legacy_organization.clone(),
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
        )
        .await?;
        Self::serve_platform(settings, listen, platform, None, None).await
    }

    /// Serve the public routes on TCP and the private sign-in link on its own
    /// Unix socket. The public listener never mounts the link's routes.
    pub async fn start_with_sign_in(
        settings: TokioSettings,
        listen: SocketAddr,
        sign_in: crate::SignInLinkSettings,
    ) -> Result<Self, TokioServerError> {
        let platform = TokioCentralPlatform::new(
            &settings.jwks_url,
            Arc::new(SystemClock),
            crate::native_logging::observe,
            &settings.root,
            settings.legacy_organization.clone(),
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
        )
        .await?;
        Self::serve_platform(settings, listen, platform, None, Some(sign_in)).await
    }

    /// Publishes only public keys, outside the authenticated application routes.
    pub async fn start_with_jwks(
        settings: TokioSettings,
        listen: SocketAddr,
        keys: collaboration_central_core::AccessJwks,
    ) -> Result<Self, TokioServerError> {
        let platform = TokioCentralPlatform::new(
            &settings.jwks_url,
            Arc::new(SystemClock),
            crate::native_logging::observe,
            &settings.root,
            settings.legacy_organization.clone(),
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
        )
        .await?;
        Self::serve_platform(settings, listen, platform, Some(keys), None).await
    }

    #[cfg(feature = "conformance")]
    pub async fn start_with_controls(
        settings: TokioSettings,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
    ) -> Result<Self, TokioServerError> {
        let clock: Arc<dyn runtime_primitives::Clock> = Arc::new(SystemClock);
        let platform = TokioCentralPlatform::new_with_controls(
            &settings.jwks_url,
            clock.clone(),
            crate::native_logging::observe,
            &settings.root,
            settings.legacy_organization.clone(),
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
            controls,
            clock,
        )
        .await?;
        Self::serve_platform(
            settings,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            platform,
            None,
            None,
        )
        .await
    }

    #[cfg(feature = "conformance")]
    pub async fn start_with_sign_in_and_controls(
        settings: TokioSettings,
        sign_in: crate::SignInLinkSettings,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
    ) -> Result<Self, TokioServerError> {
        Self::start_with_sign_in_controls_and_fleet_clock(
            settings,
            sign_in,
            controls,
            Arc::new(SystemClock),
        )
        .await
    }

    /// Conformance only: every Fleet object, including one first created by a
    /// later provisioning, reads `fleet_clock`; Access keeps the system clock.
    /// After moving the clock, call `conformance_wake_fleets` so live Fleets
    /// re-evaluate their due work at the new time.
    #[cfg(feature = "conformance")]
    pub async fn start_with_sign_in_controls_and_fleet_clock(
        settings: TokioSettings,
        sign_in: crate::SignInLinkSettings,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
        fleet_clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Result<Self, TokioServerError> {
        let platform = TokioCentralPlatform::new_with_controls(
            &settings.jwks_url,
            Arc::new(SystemClock),
            crate::native_logging::observe,
            &settings.root,
            settings.legacy_organization.clone(),
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
            controls,
            fleet_clock,
        )
        .await?;
        Self::serve_platform(
            settings,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            platform,
            None,
            Some(sign_in),
        )
        .await
    }

    async fn serve_platform(
        settings: TokioSettings,
        listen: SocketAddr,
        platform: TokioCentralPlatform,
        keys: Option<collaboration_central_core::AccessJwks>,
        sign_in: Option<crate::SignInLinkSettings>,
    ) -> Result<Self, TokioServerError> {
        let link_socket = sign_in.as_ref().map(|sign_in| sign_in.socket.clone());
        let link_listener = match sign_in {
            Some(sign_in) => match Self::bind_link(&platform, sign_in) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    platform.shutdown().await?;
                    return Err(error);
                }
            },
            None => None,
        };
        let application = match CentralApplication::new(
            platform.clone(),
            settings.access_policy.clone(),
            Arc::new(AccessKeyCache::default()),
            platform.legacy_organization.clone(),
        ) {
            Ok(application) => application,
            Err(error) => {
                platform.shutdown().await?;
                return Err(error.into());
            }
        };
        #[cfg(feature = "conformance")]
        let conformance_policy = settings.access_policy;
        let listener = match tokio::net::TcpListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                platform.shutdown().await?;
                return Err(error.into());
            }
        };
        let base_url = format!("http://{}", listener.local_addr()?);
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let router = central_router(application);
        let router = if let Some(keys) = keys {
            collaboration_central_core::central_jwks_router(keys).merge(router)
        } else {
            router
        };
        #[cfg(feature = "conformance")]
        let router =
            crate::conformance_control::mount(router, platform.clone(), conformance_policy)?;
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        let link = link_listener.map(|listener| {
            let router = collaboration_central_core::organization_link_router(platform.clone());
            let stopped = stop.clone();
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(stopped.cancelled_owned())
                    .await
            })
        });
        Ok(Self {
            base_url,
            platform,
            stop,
            task: Some(task),
            link,
            link_socket,
        })
    }

    /// Bind the link socket owner-only; a stale socket file is not reused.
    fn bind_link(
        platform: &TokioCentralPlatform,
        sign_in: crate::SignInLinkSettings,
    ) -> Result<tokio::net::UnixListener, TokioServerError> {
        use std::os::unix::fs::PermissionsExt;
        let socket = sign_in.socket.clone();
        let trust = crate::sign_in_trust::NativeSignInTrust::new(sign_in)
            .map_err(|error| TokioServerError::Listener(std::io::Error::other(error)))?;
        platform.install_sign_in_trust(Arc::new(trust))?;
        let listener = tokio::net::UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[cfg(feature = "conformance")]
    pub fn conformance_platform(&self) -> &TokioCentralPlatform {
        &self.platform
    }

    pub async fn shutdown(mut self) -> Result<(), TokioServerError> {
        self.stop.cancel();
        self.platform.begin_shutdown();
        // Finish HTTP requests before draining upgrades and closing their store.
        let served = match self.task.take() {
            Some(task) => task.await,
            None => Ok(Ok(())),
        };
        let linked = match self.link.take() {
            Some(task) => task.await,
            None => Ok(Ok(())),
        };
        if let Some(socket) = self.link_socket.take() {
            match std::fs::remove_file(&socket) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let stopped = self.platform.shutdown().await;
        served??;
        linked??;
        stopped?;
        Ok(())
    }
}

impl Drop for TokioServer {
    fn drop(&mut self) {
        self.stop.cancel();
        self.platform.begin_shutdown();
    }
}
