//! Lifecycle retention: the loop sleeps until the journal commits or the first moment
//! expiry may remove a record or a checkpointed address, and expires history only then.
//!
//! It is written the way collaboration-runtime's wake-driven worker loop is, which this
//! crate can't use because collaboration-runtime depends on it. One pass marks the commit
//! signal seen, reads the due time, expires history while it is due, and goes again at
//! once if the journal committed meanwhile. Then it sleeps on the commit signal plus one
//! timer at the due time; with nothing to expire it holds no timer. A failed pass retries
//! from 1 s, doubling to 30 s, logs once per run of failures, and the next pass that
//! succeeds clears the retry.
use super::{LifecycleStore, lifecycle_maintenance_error_class};
use crate::JournalError;
use runtime_primitives::{Clock, SyncTime};
use sqlite_storage_maintenance::FailedPassBackoff;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
enum RetentionPassError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// The due read and expiry share one rule; a due journal whose expiry removed nothing
    /// retries on the backoff instead of re-arming a timer in the past.
    #[error("lifecycle retention was due but expiry removed nothing")]
    DueButNothingRemoved,
}

impl RetentionPassError {
    const fn error_class(&self) -> &'static str {
        match self {
            Self::Journal(error) => lifecycle_maintenance_error_class(error),
            Self::DueButNothingRemoved => "due_but_nothing_removed",
        }
    }
}

impl LifecycleStore {
    /// Internal retention maintenance only; this does not schedule agent work. Startup is
    /// the first pass: it expires whatever is already due.
    pub async fn run_maintenance(
        &self,
        clock: Arc<dyn Clock>,
        shutdown: CancellationToken,
    ) -> Result<(), JournalError> {
        self.run_retention(&*clock, &shutdown, &RetentionObservation::default())
            .await;
        Ok(())
    }

    async fn run_retention(
        &self,
        clock: &dyn Clock,
        shutdown: &CancellationToken,
        observation: &RetentionObservation,
    ) {
        // Subscribed before the first read, so no commit between a read and the sleep is lost.
        let mut commits = self.commits.subscribe();
        let mut failures = FailedPassBackoff::default();
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            commits.borrow_and_update();
            let now = clock.now();
            let deadline = match self.expire_due_history(now, observation).await {
                Ok(next_due) => {
                    failures.record_success();
                    next_due
                }
                Err(error) => {
                    // A failed journal read or expiry marks the journal unavailable, which
                    // signals its readers. The retry runs the next pass, not that signal,
                    // and it reads whatever was written meanwhile.
                    commits.borrow_and_update();
                    let retry = failures.record_failure();
                    if retry.starts_run {
                        tracing::warn!(
                            component = "lifecycle_store",
                            maintenance_class = "lifecycle_journal_retention",
                            error_class = error.error_class(),
                            "lifecycle_observation.maintenance_degraded"
                        );
                    }
                    Some(now.after(retry.delay))
                }
            };
            if commits.has_changed().unwrap_or(false) {
                continue;
            }
            observation.armed(deadline);
            let wait = deadline.map(|due| {
                let millis = due.unix_millis().saturating_sub(clock.now().unix_millis());
                Duration::from_millis(u64::try_from(millis).unwrap_or(0))
            });
            let timer = async move {
                match wait {
                    Some(wait) => tokio::time::sleep(wait).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                () = timer => {},
                // The store owns the sender, so the signal closes only with the store.
                Ok(()) = commits.changed() => {},
            }
        }
    }

    /// Expires history while some is due at `now`, then returns when more becomes due.
    async fn expire_due_history(
        &self,
        now: SyncTime,
        observation: &RetentionObservation,
    ) -> Result<Option<SyncTime>, RetentionPassError> {
        let now_ms = now.unix_millis();
        loop {
            observation.due_read();
            match self.retention_due_ms(now_ms).await? {
                Some(due_ms) if due_ms <= now_ms => {}
                later => return Ok(later.map(SyncTime::from_unix_millis)),
            }
            observation.expiry();
            if !self
                .expire_history_reporting_removal(now_ms.div_euclid(1_000))
                .await?
            {
                return Err(RetentionPassError::DueButNothingRemoved);
            }
        }
    }
}

/// What the retention loop reports about itself. Production keeps nothing; a test sees
/// each due read and expiry, every sleep and the deadline armed before it.
#[derive(Default)]
struct RetentionObservation {
    #[cfg(test)]
    observed: observed::ObservedRetention,
}

impl RetentionObservation {
    fn due_read(&self) {
        #[cfg(test)]
        self.observed.update(|snapshot| snapshot.due_reads += 1);
    }

    fn expiry(&self) {
        #[cfg(test)]
        self.observed.update(|snapshot| snapshot.expiries += 1);
    }

    fn armed(&self, deadline: Option<SyncTime>) {
        #[cfg(test)]
        self.observed.update(|snapshot| {
            snapshot.armed = deadline;
            snapshot.sleeps += 1;
        });
        #[cfg(not(test))]
        let _ = deadline;
    }
}

#[cfg(test)]
mod observed {
    use runtime_primitives::SyncTime;
    use tokio::sync::watch;

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(super) struct RetentionSnapshot {
        pub(super) due_reads: u64,
        pub(super) expiries: u64,
        pub(super) sleeps: u64,
        /// The deadline armed before the latest sleep.
        pub(super) armed: Option<SyncTime>,
    }

    pub(super) struct ObservedRetention(watch::Sender<RetentionSnapshot>);

    impl Default for ObservedRetention {
        fn default() -> Self {
            Self(watch::Sender::new(RetentionSnapshot::default()))
        }
    }

    impl ObservedRetention {
        pub(super) fn update(&self, change: impl FnOnce(&mut RetentionSnapshot)) {
            self.0.send_modify(change);
        }

        pub(super) fn snapshot(&self) -> RetentionSnapshot {
            self.0.borrow().clone()
        }

        /// Returns once the loop has gone to sleep at least `sleeps` times.
        pub(super) async fn asleep_at_least(&self, sleeps: u64) {
            let mut receiver = self.0.subscribe();
            // The observation owns the sender, so the wait fails only if it is dropped.
            let _ = receiver
                .wait_for(|snapshot| snapshot.sleeps >= sleeps)
                .await
                .is_ok();
        }
    }
}

#[cfg(test)]
#[path = "retention_worker_test.rs"]
mod tests;
