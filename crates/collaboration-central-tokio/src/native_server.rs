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
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
        )
        .await?;
        Self::serve_platform(settings, listen, platform).await
    }

    #[cfg(feature = "conformance")]
    pub async fn start_with_controls(
        settings: TokioSettings,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
    ) -> Result<Self, TokioServerError> {
        let platform = TokioCentralPlatform::new_with_controls(
            &settings.jwks_url,
            Arc::new(SystemClock),
            crate::native_logging::observe,
            &settings.root,
            settings.registry,
            settings.delivery,
            settings.holder_answer_timeout,
            settings.copy_ack_wait,
            controls,
        )
        .await?;
        Self::serve_platform(settings, SocketAddr::from(([127, 0, 0, 1], 0)), platform).await
    }

    async fn serve_platform(
        settings: TokioSettings,
        listen: SocketAddr,
        platform: TokioCentralPlatform,
    ) -> Result<Self, TokioServerError> {
        let application = match CentralApplication::new(
            platform.clone(),
            settings.access_policy.clone(),
            Arc::new(AccessKeyCache::default()),
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
        #[cfg(feature = "conformance")]
        let router =
            crate::conformance_control::mount(router, platform.clone(), conformance_policy)?;
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            base_url,
            platform,
            stop,
            task: Some(task),
        })
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
        let stopped = self.platform.shutdown().await;
        served??;
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
