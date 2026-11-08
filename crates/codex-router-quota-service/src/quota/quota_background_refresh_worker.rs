#[cfg(any(test, feature = "test-support"))]
use super::current_unix_seconds;
use super::{
    Arc, AsyncProviderCredentialResolver, Duration, Instant, JoinHandle, PathBuf,
    QuotaRefreshObservationContext, QuotaRefreshProvider, QuotaRefreshSchedule, QuotaServiceError,
    WebSocketQuotaFloorNotifier, WeeklyQuotaFloorIntentObserver,
    refresh_quota_store_paths_with_dependencies_and_floor_notifier, thread,
};
use std::sync::mpsc;

/// Stoppable background quota refresh worker.
pub struct BackgroundQuotaRefreshWorker {
    /// Dropping it disconnects the worker's wait between cycles, which ends the thread at once.
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

pub struct BackgroundQuotaRefreshRuntime<C, D> {
    observed_clock: C,
    diagnostic_reporter: D,
    interval: Duration,
    quota_floor_notifier: Option<Arc<dyn WeeklyQuotaFloorIntentObserver>>,
    /// Signalled each time the thread enters its wait between cycles; the thread owns it,
    /// so the receiver sees it disconnect when the thread exits.
    #[cfg(test)]
    stop_wait_probe: Option<mpsc::Sender<()>>,
}

impl<C, D> BackgroundQuotaRefreshRuntime<C, D> {
    pub const fn new(observed_clock: C, diagnostic_reporter: D, interval: Duration) -> Self {
        Self {
            observed_clock,
            diagnostic_reporter,
            interval,
            quota_floor_notifier: None,
            #[cfg(test)]
            stop_wait_probe: None,
        }
    }

    #[cfg(test)]
    fn with_stop_wait_probe(mut self, probe: mpsc::Sender<()>) -> Self {
        self.stop_wait_probe = Some(probe);
        self
    }

    pub fn with_quota_floor_notifier(
        mut self,
        quota_floor_notifier: WebSocketQuotaFloorNotifier,
    ) -> Self {
        self.quota_floor_notifier = Some(Arc::new(quota_floor_notifier));
        self
    }
}

impl Drop for BackgroundQuotaRefreshWorker {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _result = thread.join();
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn start_background_quota_refresh_worker_with_dependencies<R, P>(
    state_db: PathBuf,
    secret_root: PathBuf,
    base_url: String,
    credential_resolver: R,
    quota_provider: P,
    interval: Duration,
) -> BackgroundQuotaRefreshWorker
where
    R: AsyncProviderCredentialResolver + Send + 'static,
    P: QuotaRefreshProvider + Send + 'static,
{
    start_background_quota_refresh_worker_with_clock(
        state_db,
        secret_root,
        base_url,
        credential_resolver,
        quota_provider,
        current_unix_seconds,
        interval,
    )
}

#[cfg(any(test, feature = "test-support"))]
pub fn start_background_quota_refresh_worker_with_clock<R, P, C>(
    state_db: PathBuf,
    secret_root: PathBuf,
    base_url: String,
    credential_resolver: R,
    quota_provider: P,
    observed_clock: C,
    interval: Duration,
) -> BackgroundQuotaRefreshWorker
where
    R: AsyncProviderCredentialResolver + Send + 'static,
    P: QuotaRefreshProvider + Send + 'static,
    C: FnMut() -> u64 + Send + 'static,
{
    start_background_quota_refresh_worker_with_reporter(
        state_db,
        secret_root,
        base_url,
        credential_resolver,
        quota_provider,
        BackgroundQuotaRefreshRuntime::new(observed_clock, |_diagnostic| {}, interval),
    )
}

pub fn start_background_quota_refresh_worker_with_reporter<R, P, C, D>(
    state_db: PathBuf,
    secret_root: PathBuf,
    base_url: String,
    credential_resolver: R,
    quota_provider: P,
    runtime: BackgroundQuotaRefreshRuntime<C, D>,
) -> BackgroundQuotaRefreshWorker
where
    R: AsyncProviderCredentialResolver + Send + 'static,
    P: QuotaRefreshProvider + Send + 'static,
    C: FnMut() -> u64 + Send + 'static,
    D: FnMut(String) + Send + 'static,
{
    let BackgroundQuotaRefreshRuntime {
        mut observed_clock,
        mut diagnostic_reporter,
        interval,
        quota_floor_notifier,
        #[cfg(test)]
        stop_wait_probe,
    } = runtime;
    let (stop, stop_requested) = mpsc::channel::<()>();
    let thread = thread::spawn(move || {
        loop {
            let cycle_started_at = Instant::now();
            let mut sink = Vec::new();
            let observed_unix_seconds = observed_clock();
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(QuotaServiceError::BackgroundWorkerInitialization)
                .and_then(|refresh_runtime| {
                    refresh_runtime.block_on(
                        refresh_quota_store_paths_with_dependencies_and_floor_notifier(
                            &mut sink,
                            &state_db,
                            &secret_root,
                            base_url.clone(),
                            &credential_resolver,
                            &quota_provider,
                            QuotaRefreshObservationContext {
                                observed_unix_seconds,
                                schedule: QuotaRefreshSchedule::Background {
                                    interval_seconds: interval.as_secs(),
                                },
                                weekly_floor_observer: quota_floor_notifier.as_deref(),
                            },
                        ),
                    )
                });
            let diagnostic_output = String::from_utf8_lossy(&sink).into_owned();
            if diagnostic_output
                .lines()
                .any(|line| line.starts_with("refresh failed:") || line.starts_with("failed:"))
            {
                diagnostic_reporter(diagnostic_output.trim_end().to_owned());
            }
            if let Err(error) = result {
                diagnostic_reporter(format!("background quota refresh failed: {error}"));
            }
            if interval.is_zero() {
                break;
            }
            let delay = refresh_cycle_delay(interval, cycle_started_at.elapsed());
            #[cfg(test)]
            if let Some(probe) = &stop_wait_probe {
                let _ = probe.send(());
            }
            if !matches!(
                stop_requested.recv_timeout(delay),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                break;
            }
        }
    });

    BackgroundQuotaRefreshWorker {
        stop: Some(stop),
        thread: Some(thread),
    }
}

pub fn refresh_cycle_delay(interval: Duration, elapsed: Duration) -> Duration {
    interval.saturating_sub(elapsed)
}

#[cfg(test)]
#[path = "quota_background_refresh_worker_test.rs"]
mod tests;
