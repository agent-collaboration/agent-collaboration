//! A supplied Clock around the same native capabilities and core HTTP router.

use collaboration_central_conformance::{ConformanceResult, fixture_policy};
use collaboration_central_core::{
    AccessKeyCache, CentralApplication, RegistryTime, central_router,
};
use collaboration_central_tokio::TokioCentralPlatform;
use runtime_primitives::{Clock, SyncTime};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use tokio_util::sync::CancellationToken;

#[path = "../native_http/support.rs"]
pub(super) mod configuration;

#[derive(Clone)]
pub struct SuppliedClock(Arc<AtomicI64>);
impl Clock for SuppliedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
impl SuppliedClock {
    pub fn set_time(&self, at: RegistryTime) {
        self.0.store(at.0.timestamp_millis(), Ordering::SeqCst);
    }
}

pub struct ClockedServer {
    base: String,
    pub clock: SuppliedClock,
    platform: TokioCentralPlatform,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl ClockedServer {
    pub async fn start(root: &std::path::Path, jwks: &str) -> ConformanceResult<Self> {
        let clock = SuppliedClock(Arc::new(AtomicI64::new(
            runtime_primitives::SystemClock.now().unix_millis(),
        )));
        let platform = TokioCentralPlatform::new(
            jwks,
            Arc::new(clock.clone()),
            |_| {},
            root,
            configuration::registry_config()?,
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            std::time::Duration::from_secs(2),
            std::time::Duration::from_secs(2),
        )
        .await?;
        let application = match CentralApplication::new(
            platform.clone(),
            fixture_policy(),
            Arc::new(AccessKeyCache::default()),
        ) {
            Ok(application) => application,
            Err(error) => {
                platform.shutdown().await?;
                return Err(error.into());
            }
        };
        let listener = match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await
        {
            Ok(listener) => listener,
            Err(error) => {
                platform.shutdown().await?;
                return Err(error.into());
            }
        };
        let base = format!("http://{}", listener.local_addr()?);
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, central_router(application))
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            base,
            clock,
            platform,
            stop,
            task,
        })
    }
    pub fn base_url(&self) -> &str {
        &self.base
    }
    pub async fn shutdown(self) -> ConformanceResult<()> {
        self.stop.cancel();
        let objects = self.platform.shutdown().await;
        let http = self.task.await;
        objects?;
        http??;
        Ok(())
    }
}
