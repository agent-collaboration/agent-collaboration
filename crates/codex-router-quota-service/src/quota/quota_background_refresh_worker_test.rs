use super::{BackgroundQuotaRefreshRuntime, start_background_quota_refresh_worker_with_reporter};
use crate::credential_resolution::AsyncProviderCredentialResolver;
use crate::quota::{
    QuotaRefreshProvider, QuotaRefreshProviderRequest, QuotaRefreshProviderResponse,
};
use crate::quota_service_error::QuotaServiceError;
use codex_router_auth::resolver::{CredentialResolverError, ResolvedProviderCredential};
use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use std::sync::mpsc;
use std::time::Duration;

/// The worker's cycle fails before it needs credentials: its state database does not exist.
struct UnusedCredentials;
impl AsyncProviderCredentialResolver for UnusedCredentials {
    async fn resolve_provider_credentials_async(
        &self,
        _account_id: &AccountId,
        _expected_provider: Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError> {
        Err(CredentialResolverError::RefreshUnavailable)
    }
}

struct UnusedProvider;
impl QuotaRefreshProvider for UnusedProvider {
    async fn fetch_quota(
        &self,
        _request: QuotaRefreshProviderRequest,
    ) -> Result<QuotaRefreshProviderResponse, QuotaServiceError> {
        Err(QuotaServiceError::ProviderStatus { status: 500 })
    }
}

/// The receive bounds only stop a hang; the handshake is the signal.
const HANG_GUARD: Duration = Duration::from_secs(10);

#[test]
fn dropping_the_worker_ends_its_hour_long_wait_through_the_stop_channel() {
    let root = tempfile::tempdir().expect("isolated worker root");
    let (entered_wait, entered_wait_seen) = mpsc::channel();
    let worker = start_background_quota_refresh_worker_with_reporter(
        root.path().join("missing-state.sqlite"),
        root.path().join("secrets"),
        "https://chatgpt.com/backend-api".to_owned(),
        UnusedCredentials,
        UnusedProvider,
        BackgroundQuotaRefreshRuntime::new(|| 0, |_diagnostic| {}, Duration::from_secs(3_600))
            .with_stop_wait_probe(entered_wait),
    );
    entered_wait_seen
        .recv_timeout(HANG_GUARD)
        .expect("the worker entered its wait between cycles");

    let (dropped, dropped_seen) = mpsc::channel();
    std::thread::spawn(move || {
        drop(worker);
        let _ = dropped.send(());
    });

    dropped_seen
        .recv_timeout(HANG_GUARD)
        .expect("dropping the worker stopped its hour-long wait");
    assert_eq!(
        entered_wait_seen.recv_timeout(HANG_GUARD),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "the worker thread exited without starting another cycle"
    );
}
