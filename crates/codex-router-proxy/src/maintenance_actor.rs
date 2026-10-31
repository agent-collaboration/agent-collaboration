//! Bounded actor for SQLite maintenance that must never gate socket progress.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::sync::mpsc::Sender as TestCompletionSender;

use codex_router_core::routes::RouteBand;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use codex_router_state::sqlite::PREVIOUS_RESPONSE_AFFINITY_OWNER_PRUNE_BATCH_SIZE;
use codex_router_state::sqlite::QUOTA_HISTORY_OBSERVATION_PRUNE_BATCH_SIZE;
use codex_router_state::sqlite::StateStoreError;
use futures_util::future::BoxFuture;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::telemetry::record_maintenance_lag_observed;

/// Default bounded capacity for coalesced maintenance hints.
pub const MAINTENANCE_QUEUE_CAPACITY: usize = 32;

const PREVIOUS_RESPONSE_AFFINITY_OWNER_MAX_BATCHES_PER_PASS: usize = 4_096;
const QUOTA_HISTORY_OBSERVATION_MAX_BATCHES_PER_PASS: usize = 128;

/// Retains previous-response affinity owners for 30 days.
const PREVIOUS_RESPONSE_AFFINITY_OWNER_RETENTION_SECONDS: u64 = 30 * 86_400;

/// Retains session-account pins for one week after their last observation.
pub const SESSION_ACCOUNT_AFFINITY_RETENTION_SECONDS: u64 = 7 * 86_400;

/// Retains active-session rollup buckets for one week.
pub const ACTIVE_SESSION_ROLLUP_RETENTION_SECONDS: u64 = 7 * 86_400;

pub(crate) const fn previous_response_affinity_owner_retention_cutoff(
    now_unix_seconds: u64,
) -> u64 {
    now_unix_seconds.saturating_sub(PREVIOUS_RESPONSE_AFFINITY_OWNER_RETENTION_SECONDS)
}

pub(crate) const fn session_account_affinity_retention_cutoff(now_unix_seconds: u64) -> u64 {
    now_unix_seconds.saturating_sub(SESSION_ACCOUNT_AFFINITY_RETENTION_SECONDS)
}

pub(crate) const fn active_session_rollup_retention_cutoff(now_unix_seconds: u64) -> u64 {
    now_unix_seconds.saturating_sub(ACTIVE_SESSION_ROLLUP_RETENTION_SECONDS)
}

/// Result of a non-blocking maintenance hint enqueue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaintenanceEnqueueResult {
    /// Hint entered the bounded queue.
    Enqueued,
    /// Duplicate hint is already queued or running.
    Coalesced,
    /// Queue had no capacity and maintenance is degraded.
    FullDegraded,
    /// Actor is closed and maintenance is degraded.
    ClosedDegraded,
}

/// Coalescible maintenance hint.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceHint {
    /// Clean up session-account affinities and quota history before the weekly cutoff.
    CleanupStaleSessionAccountAffinities { stale_before_unix_seconds: u64 },
    /// Cleanup active-client leases older than the cutoff for one route band.
    CleanupStaleActiveClients {
        route_band: RouteBand,
        stale_before_unix_seconds: u64,
    },
    /// Refresh active-session rollups for one interval.
    RefreshActiveSessionRollups {
        route_band: RouteBand,
        interval_start_unix_seconds: u64,
        interval_end_unix_seconds: u64,
        bucket_seconds: u64,
    },
    /// Apply retention to persisted active-session rollup buckets for one route band.
    ApplyActiveSessionRetention {
        route_band: RouteBand,
        retain_after_unix_seconds: u64,
    },
    /// Run active-session history compaction for one route band.
    CompactActiveSessionHistory {
        route_band: RouteBand,
        compact_before_unix_seconds: u64,
    },
    /// Delete one batch of previous-response owners created before the cutoff.
    PrunePreviousResponseAffinityOwners { created_before_unix_seconds: u64 },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaintenanceCompletion {
    maintenance_class: &'static str,
    route_band: &'static str,
}

#[cfg(test)]
impl MaintenanceCompletion {
    #[must_use]
    pub(crate) const fn maintenance_class(self) -> &'static str {
        self.maintenance_class
    }

    #[must_use]
    pub(crate) const fn route_band(self) -> &'static str {
        self.route_band
    }
}

#[derive(Clone, Debug)]
struct QueuedMaintenanceHint {
    hint: MaintenanceHint,
    enqueued_at: Instant,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum MaintenanceCoalescingKey {
    GlobalClass {
        maintenance_class: &'static str,
    },
    RouteBandClass {
        route_band: RouteBand,
        maintenance_class: &'static str,
    },
    RollupInterval {
        route_band: RouteBand,
        interval_start_unix_seconds: u64,
        interval_end_unix_seconds: u64,
        bucket_seconds: u64,
    },
}

/// Maintenance repository boundary.
pub trait MaintenanceRepository: Send + Sync + 'static {
    /// Runs one maintenance hint.
    fn run_maintenance_hint<'a>(
        &'a self,
        hint: MaintenanceHint,
    ) -> BoxFuture<'a, Result<(), MaintenanceRepositoryError>>;
}

impl<T> MaintenanceRepository for Arc<T>
where
    T: MaintenanceRepository,
{
    fn run_maintenance_hint<'a>(
        &'a self,
        hint: MaintenanceHint,
    ) -> BoxFuture<'a, Result<(), MaintenanceRepositoryError>> {
        self.as_ref().run_maintenance_hint(hint)
    }
}

/// Maintenance repository failure.
#[derive(Debug, Error)]
pub enum MaintenanceRepositoryError {
    /// SQLite/state failed while running maintenance.
    #[error("state store unavailable while running maintenance")]
    State(#[from] StateStoreError),
}

impl MaintenanceRepository for AsyncSqliteStateStore {
    fn run_maintenance_hint<'a>(
        &'a self,
        hint: MaintenanceHint,
    ) -> BoxFuture<'a, Result<(), MaintenanceRepositoryError>> {
        Box::pin(async move {
            let mut should_vacuum = false;
            match hint {
                MaintenanceHint::CleanupStaleSessionAccountAffinities {
                    stale_before_unix_seconds,
                } => {
                    self.purge_session_account_affinities_before(stale_before_unix_seconds)
                        .await?;
                    let mut rows_deleted = 0_u64;
                    let mut batch_count = 0_u64;
                    for _batch_index in 0..QUOTA_HISTORY_OBSERVATION_MAX_BATCHES_PER_PASS {
                        let deleted_in_batch = self
                            .prune_quota_history_observations_before(
                                stale_before_unix_seconds,
                                QUOTA_HISTORY_OBSERVATION_PRUNE_BATCH_SIZE,
                            )
                            .await?;
                        rows_deleted = rows_deleted.saturating_add(deleted_in_batch);
                        batch_count = batch_count.saturating_add(1);
                        if deleted_in_batch < u64::from(QUOTA_HISTORY_OBSERVATION_PRUNE_BATCH_SIZE)
                        {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                    tracing::info!(
                        maintenance.class = "quota_history_retention",
                        route_band = "all",
                        maintenance.rows_deleted = rows_deleted,
                        maintenance.batch_count = batch_count,
                        "codex_router.maintenance_completed"
                    );
                    should_vacuum = true;
                }
                MaintenanceHint::CleanupStaleActiveClients {
                    route_band,
                    stale_before_unix_seconds,
                } => {
                    self.active_client_counts_for_route_band(
                        route_band.as_str(),
                        stale_before_unix_seconds,
                        0,
                    )
                    .await?;
                    should_vacuum = true;
                }
                MaintenanceHint::RefreshActiveSessionRollups {
                    route_band,
                    interval_start_unix_seconds,
                    interval_end_unix_seconds,
                    bucket_seconds,
                } => {
                    self.refresh_active_session_rollups_for_interval(
                        route_band.as_str(),
                        interval_start_unix_seconds,
                        interval_end_unix_seconds,
                        bucket_seconds,
                    )
                    .await?;
                }
                MaintenanceHint::ApplyActiveSessionRetention {
                    retain_after_unix_seconds,
                    ..
                } => {
                    self.purge_active_session_rollups_before(retain_after_unix_seconds)
                        .await?;
                    should_vacuum = true;
                }
                MaintenanceHint::CompactActiveSessionHistory {
                    route_band,
                    compact_before_unix_seconds,
                } => {
                    self.compact_completed_active_session_events_before(
                        route_band.as_str(),
                        compact_before_unix_seconds,
                    )
                    .await?;
                    should_vacuum = true;
                }
                MaintenanceHint::PrunePreviousResponseAffinityOwners {
                    created_before_unix_seconds,
                } => {
                    let mut rows_deleted = 0_u64;
                    let mut batch_count = 0_u64;
                    for _batch_index in 0..PREVIOUS_RESPONSE_AFFINITY_OWNER_MAX_BATCHES_PER_PASS {
                        let deleted_in_batch = self
                            .prune_previous_response_owners_before(created_before_unix_seconds)
                            .await?;
                        rows_deleted = rows_deleted.saturating_add(deleted_in_batch);
                        batch_count = batch_count.saturating_add(1);
                        let batch_was_full = i64::try_from(deleted_in_batch).ok()
                            == Some(PREVIOUS_RESPONSE_AFFINITY_OWNER_PRUNE_BATCH_SIZE);
                        if !batch_was_full {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                    tracing::info!(
                        maintenance.class = "previous_response_affinity_owner_retention",
                        route_band = "all",
                        maintenance.rows_deleted = rows_deleted,
                        maintenance.batch_count = batch_count,
                        "codex_router.maintenance_completed"
                    );
                    should_vacuum = true;
                }
            }
            if should_vacuum {
                self.incremental_vacuum_step().await?;
            }
            Ok(())
        })
    }
}

/// Bounded maintenance actor handle.
#[derive(Clone, Debug)]
pub struct MaintenanceActor {
    sender: mpsc::Sender<QueuedMaintenanceHint>,
    pending: Arc<Mutex<HashMap<MaintenanceCoalescingKey, Instant>>>,
    closed: Arc<AtomicBool>,
    shutdown: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    #[cfg(test)]
    completion_sender: Arc<Mutex<Option<TestCompletionSender<MaintenanceCompletion>>>>,
}

impl MaintenanceActor {
    /// Starts a bounded maintenance actor.
    #[must_use]
    pub fn start<R>(repository: R, capacity: usize) -> Self
    where
        R: MaintenanceRepository,
    {
        let repository = Arc::new(repository);
        Self::start_on_handle(&Handle::current(), repository, capacity)
    }

    /// Starts a bounded maintenance actor from a shared repository.
    #[must_use]
    pub fn start_with_repository(
        repository: Arc<dyn MaintenanceRepository>,
        capacity: usize,
    ) -> Self {
        Self::start_on_handle(&Handle::current(), repository, capacity)
    }

    /// Starts a bounded maintenance actor from a shared repository on an explicit runtime handle.
    #[must_use]
    pub fn start_on_handle(
        runtime_handle: &Handle,
        repository: Arc<dyn MaintenanceRepository>,
        capacity: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let shutdown = CancellationToken::new();
        let task_pending = Arc::clone(&pending);
        let task_closed = Arc::clone(&closed);
        let task_shutdown = shutdown.clone();
        #[cfg(test)]
        let completion_sender = Arc::new(Mutex::new(None));
        #[cfg(test)]
        let task_completion_sender = Arc::clone(&completion_sender);
        let task = runtime_handle.spawn(async move {
            run_maintenance_actor(
                repository,
                receiver,
                task_pending,
                task_shutdown,
                task_closed,
                #[cfg(test)]
                task_completion_sender,
            )
            .await;
        });
        Self {
            sender,
            pending,
            closed,
            shutdown,
            task: Arc::new(Mutex::new(Some(task))),
            #[cfg(test)]
            completion_sender,
        }
    }

    #[cfg(test)]
    pub(crate) fn register_completion_sender(
        &self,
        completion_sender: TestCompletionSender<MaintenanceCompletion>,
    ) {
        match self.completion_sender.lock() {
            Ok(mut registered_sender) => {
                *registered_sender = Some(completion_sender);
            }
            Err(error) => panic!("maintenance completion sender lock should be available: {error}"),
        }
    }

    /// Attempts to enqueue without waiting on maintenance work.
    #[must_use]
    pub fn try_enqueue(&self, hint: MaintenanceHint) -> MaintenanceEnqueueResult {
        let enqueued_at = Instant::now();
        if self.closed.load(Ordering::Acquire) {
            record_maintenance_lag_observed(
                hint.maintenance_class(),
                hint.route_band_label(),
                "degraded",
                0,
            );
            return MaintenanceEnqueueResult::ClosedDegraded;
        }
        let coalescing_key = hint.coalescing_key();
        {
            let mut pending = match self.pending.lock() {
                Ok(pending) => pending,
                Err(error) => {
                    tracing::warn!(
                        component = "maintenance_actor",
                        lock = "pending_set",
                        error.message = %error,
                        "codex_router.maintenance_pending_set_lock_poisoned"
                    );
                    record_maintenance_lag_observed(
                        hint.maintenance_class(),
                        hint.route_band_label(),
                        "degraded",
                        0,
                    );
                    return MaintenanceEnqueueResult::ClosedDegraded;
                }
            };
            if let Some(existing_enqueued_at) = pending.get(&coalescing_key) {
                record_maintenance_lag_observed(
                    hint.maintenance_class(),
                    hint.route_band_label(),
                    "coalesced",
                    lag_millis_since(*existing_enqueued_at),
                );
                return MaintenanceEnqueueResult::Coalesced;
            }
            pending.insert(coalescing_key.clone(), enqueued_at);
        }
        let queued_hint = QueuedMaintenanceHint {
            hint: hint.clone(),
            enqueued_at,
        };
        match self.sender.try_send(queued_hint) {
            Ok(()) => MaintenanceEnqueueResult::Enqueued,
            Err(mpsc::error::TrySendError::Full(_hint)) => {
                self.remove_pending_key(&coalescing_key);
                record_maintenance_lag_observed(
                    hint.maintenance_class(),
                    hint.route_band_label(),
                    "degraded",
                    0,
                );
                MaintenanceEnqueueResult::FullDegraded
            }
            Err(mpsc::error::TrySendError::Closed(_hint)) => {
                self.remove_pending_key(&coalescing_key);
                self.closed.store(true, Ordering::Release);
                record_maintenance_lag_observed(
                    hint.maintenance_class(),
                    hint.route_band_label(),
                    "degraded",
                    0,
                );
                MaintenanceEnqueueResult::ClosedDegraded
            }
        }
    }

    /// Cancels the actor and waits for the task to stop.
    pub async fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        self.shutdown.cancel();
        let task = match self.task.lock() {
            Ok(mut task) => task.take(),
            Err(error) => {
                tracing::warn!(
                    component = "maintenance_actor",
                    lock = "task",
                    error.message = %error,
                    "codex_router.maintenance_task_lock_poisoned"
                );
                None
            }
        };
        if let Some(task) = task {
            let _join_result = task.await;
        }
    }

    fn remove_pending_key(&self, coalescing_key: &MaintenanceCoalescingKey) {
        match self.pending.lock() {
            Ok(mut pending) => {
                pending.remove(coalescing_key);
            }
            Err(error) => {
                tracing::warn!(
                    component = "maintenance_actor",
                    lock = "pending_set",
                    error.message = %error,
                    "codex_router.maintenance_pending_remove_lock_poisoned"
                );
            }
        }
    }
}

impl MaintenanceHint {
    fn coalescing_key(&self) -> MaintenanceCoalescingKey {
        match self {
            Self::CleanupStaleSessionAccountAffinities { .. } => {
                MaintenanceCoalescingKey::GlobalClass {
                    maintenance_class: self.maintenance_class(),
                }
            }
            Self::PrunePreviousResponseAffinityOwners { .. } => {
                MaintenanceCoalescingKey::GlobalClass {
                    maintenance_class: self.maintenance_class(),
                }
            }
            Self::RefreshActiveSessionRollups {
                route_band,
                interval_start_unix_seconds,
                interval_end_unix_seconds,
                bucket_seconds,
            } => MaintenanceCoalescingKey::RollupInterval {
                route_band: *route_band,
                interval_start_unix_seconds: *interval_start_unix_seconds,
                interval_end_unix_seconds: *interval_end_unix_seconds,
                bucket_seconds: *bucket_seconds,
            },
            Self::CleanupStaleActiveClients { route_band, .. }
            | Self::ApplyActiveSessionRetention { route_band, .. }
            | Self::CompactActiveSessionHistory { route_band, .. } => {
                MaintenanceCoalescingKey::RouteBandClass {
                    route_band: *route_band,
                    maintenance_class: self.maintenance_class(),
                }
            }
        }
    }

    const fn maintenance_class(&self) -> &'static str {
        match self {
            Self::CleanupStaleSessionAccountAffinities { .. } => {
                "stale_session_account_affinity_cleanup"
            }
            Self::CleanupStaleActiveClients { .. } => "stale_active_client_cleanup",
            Self::RefreshActiveSessionRollups { .. } => "active_session_rollup_refresh",
            Self::ApplyActiveSessionRetention { .. } => "active_session_retention",
            Self::CompactActiveSessionHistory { .. } => "active_session_history_compaction",
            Self::PrunePreviousResponseAffinityOwners { .. } => {
                "previous_response_affinity_owner_retention"
            }
        }
    }

    const fn route_band_label(&self) -> &'static str {
        match self {
            Self::CleanupStaleSessionAccountAffinities { .. }
            | Self::PrunePreviousResponseAffinityOwners { .. } => "all",
            Self::CleanupStaleActiveClients { route_band, .. }
            | Self::RefreshActiveSessionRollups { route_band, .. }
            | Self::ApplyActiveSessionRetention { route_band, .. }
            | Self::CompactActiveSessionHistory { route_band, .. } => route_band.as_str(),
        }
    }
}

async fn run_maintenance_actor(
    repository: Arc<dyn MaintenanceRepository>,
    mut receiver: mpsc::Receiver<QueuedMaintenanceHint>,
    pending: Arc<Mutex<HashMap<MaintenanceCoalescingKey, Instant>>>,
    shutdown: CancellationToken,
    closed: Arc<AtomicBool>,
    #[cfg(test)] completion_sender: Arc<Mutex<Option<TestCompletionSender<MaintenanceCompletion>>>>,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                receiver.close();
                break;
            }
            queued_hint = receiver.recv() => {
                let Some(queued_hint) = queued_hint else {
                    break;
                };
                let QueuedMaintenanceHint { hint, enqueued_at } = queued_hint;
                let hint_for_cleanup = hint.clone();
                let coalescing_key = hint.coalescing_key();
                record_maintenance_lag_observed(
                    hint.maintenance_class(),
                    hint.route_band_label(),
                    "processing",
                    lag_millis_since(enqueued_at),
                );
                tokio::select! {
                    () = shutdown.cancelled() => {
                        match pending.lock() {
                            Ok(mut pending) => {
                                pending.remove(&coalescing_key);
                            }
                            Err(error) => {
                                tracing::warn!(
                                    component = "maintenance_actor",
                                    lock = "pending_set",
                                    error.message = %error,
                                    "codex_router.maintenance_pending_shutdown_lock_poisoned"
                                );
                            }
                        }
                        receiver.close();
                        break;
                    }
                    result = repository.run_maintenance_hint(hint.clone()) => {
                        if result.is_err() {
                            record_maintenance_lag_observed(
                                hint.maintenance_class(),
                                hint.route_band_label(),
                                "degraded",
                                lag_millis_since(enqueued_at),
                            );
                        }
                    }
                }
                match pending.lock() {
                    Ok(mut pending) => {
                        pending.remove(&hint_for_cleanup.coalescing_key());
                    }
                    Err(error) => {
                        tracing::warn!(
                            component = "maintenance_actor",
                            lock = "pending_set",
                            error.message = %error,
                            "codex_router.maintenance_pending_cleanup_lock_poisoned"
                        );
                    }
                }
                #[cfg(test)]
                if let Ok(completion_sender) = completion_sender.lock()
                    && let Some(completion_sender) = completion_sender.as_ref()
                {
                    let _send_result = completion_sender.send(MaintenanceCompletion {
                        maintenance_class: hint_for_cleanup.maintenance_class(),
                        route_band: hint_for_cleanup.route_band_label(),
                    });
                }
            }
        }
    }
    closed.store(true, Ordering::Release);
}

fn lag_millis_since(enqueued_at: Instant) -> u64 {
    u64::try_from(enqueued_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "maintenance_actor/maintenance_behavior_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "maintenance_actor/affinity_retention_tests.rs"]
mod affinity_retention_tests;
#[cfg(test)]
#[path = "maintenance_actor/quota_history_retention_tests.rs"]
mod quota_history_retention_tests;

#[cfg(test)]
#[path = "maintenance_actor/incremental_vacuum_tests.rs"]
mod incremental_vacuum_tests;
