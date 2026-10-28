//! Composition for one interactive quota reset session.

use std::future::Future;
#[cfg(feature = "quota-reset-test-harness")]
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;

use codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore;

use super::QuotaResetError;
use super::provider_protocol::HttpLiveQuotaResetProvider;
use super::reset_commit_service::LiveResetAuthorityReader;
use super::reset_commit_service::ResetWorkflowService;
use super::reset_session_supervisor::ProductionRedeemRequestIdFactory;
use super::reset_session_supervisor::ProductionResetClock;
use super::reset_session_supervisor::QuotaInteractiveSession;
use super::reset_session_supervisor::ResetSessionOutcome;
use super::reset_session_supervisor::ResetSessionPorts;

pub type ResetSessionRunner = Pin<Box<dyn Future<Output = ResetSessionOutcome> + Send + 'static>>;

pub struct InteractiveResetSession {
    pub ports: ResetSessionPorts,
    pub runner: ResetSessionRunner,
}

pub trait InteractiveResetSessionFactory: Send + Sync {
    fn create(
        &self,
        router_root: &Path,
        credential_store: RuntimeCredentialStore,
    ) -> Result<InteractiveResetSession, QuotaResetError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FixedOriginInteractiveResetSessionFactory;

impl InteractiveResetSessionFactory for FixedOriginInteractiveResetSessionFactory {
    fn create(
        &self,
        router_root: &Path,
        credential_store: RuntimeCredentialStore,
    ) -> Result<InteractiveResetSession, QuotaResetError> {
        compose_http_reset_session(
            router_root,
            credential_store,
            HttpLiveQuotaResetProvider::new()?,
        )
    }
}

#[cfg(feature = "quota-reset-test-harness")]
#[derive(Clone, Copy, Debug)]
pub struct LoopbackInteractiveResetSessionFactory {
    provider_listener: SocketAddr,
}

#[cfg(feature = "quota-reset-test-harness")]
impl LoopbackInteractiveResetSessionFactory {
    pub const fn new(provider_listener: SocketAddr) -> Self {
        Self { provider_listener }
    }
}

#[cfg(feature = "quota-reset-test-harness")]
impl InteractiveResetSessionFactory for LoopbackInteractiveResetSessionFactory {
    fn create(
        &self,
        router_root: &Path,
        credential_store: RuntimeCredentialStore,
    ) -> Result<InteractiveResetSession, QuotaResetError> {
        compose_http_reset_session(
            router_root,
            credential_store,
            HttpLiveQuotaResetProvider::new_loopback(self.provider_listener)?,
        )
    }
}

fn compose_http_reset_session(
    router_root: &Path,
    credential_store: codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore,
    provider: HttpLiveQuotaResetProvider,
) -> Result<InteractiveResetSession, QuotaResetError> {
    let authority_reader =
        LiveResetAuthorityReader::new(router_root.join("state.sqlite"), credential_store);
    let service = ResetWorkflowService::new(authority_reader, provider);
    let (session, ports) = QuotaInteractiveSession::new(
        service,
        ProductionRedeemRequestIdFactory,
        std::sync::Arc::new(ProductionResetClock),
    );
    Ok(InteractiveResetSession {
        ports,
        runner: Box::pin(session.run()),
    })
}
