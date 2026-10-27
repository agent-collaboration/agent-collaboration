//! CLI composition for the background quota refresher.
use super::{
    CliCredentialResolver, Duration, HttpQuotaRefreshProvider, PathBuf, QuotaCommandError,
    WebSocketQuotaFloorNotifier, current_unix_seconds,
};
use codex_router_quota_service::quota::quota_background_refresh_worker::{
    BackgroundQuotaRefreshRuntime, BackgroundQuotaRefreshWorker,
    start_background_quota_refresh_worker_with_reporter,
};
pub(crate) fn start_background_quota_refresh_worker(
    state_db: PathBuf,
    secret_root: PathBuf,
    credential_store: crate::secret_store_factory::CliRuntimeSecretStore,
    base_url: String,
    interval: Duration,
    quota_floor_notifier: WebSocketQuotaFloorNotifier,
) -> Result<BackgroundQuotaRefreshWorker, QuotaCommandError> {
    let resolver = CliCredentialResolver::open_with_secret_store(&state_db, credential_store)?;
    let provider = HttpQuotaRefreshProvider::new()?;
    Ok(start_background_quota_refresh_worker_with_reporter(
        state_db,
        secret_root,
        base_url,
        resolver,
        provider,
        BackgroundQuotaRefreshRuntime::new(
            current_unix_seconds,
            |diagnostic| eprintln!("{diagnostic}"),
            interval,
        )
        .with_quota_floor_notifier(quota_floor_notifier),
    ))
}
