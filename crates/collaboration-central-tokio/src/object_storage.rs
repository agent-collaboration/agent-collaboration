//! One object's storage on the Tokio host: its SQLite file, opened and migrated on
//! first use, and the alarm time its last committed batch asked for.

use crate::sqlite_object_store::{AlarmChange, SqliteObjectStore};
use central_storage::{BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreFailure};
use runtime_primitives::SyncTime;
use sqlx::migrate::Migrator;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard, OnceCell, watch};

/// The SQLx migrators for each object kind, run when an object's file opens.
#[derive(Clone)]
pub struct StoreMigrations {
    pub central: Arc<Migrator>,
    pub workspace: Arc<Migrator>,
}

impl StoreMigrations {
    /// Production object kinds have no migrations yet: their schemas await the owner's
    /// review (migrations/central and migrations/workspace arrive with their slices).
    #[must_use]
    pub fn production() -> Self {
        Self {
            central: Arc::new(Migrator::with_migrations(Vec::new())),
            workspace: Arc::new(Migrator::with_migrations(Vec::new())),
        }
    }
}

pub struct ObjectStorage {
    path: PathBuf,
    migrator: Arc<Migrator>,
    store: OnceCell<SqliteObjectStore>,
    // A database commit and its retained alarm are one ordered object write.
    batch_order: Mutex<()>,
    #[cfg(feature = "test-support")]
    alarm_batch_failure: std::sync::Mutex<Option<StoreFailure>>,
    /// Kept in memory after commit (Program Design, "the one alarm"; Tokio).
    alarm: watch::Sender<Option<SyncTime>>,
    #[cfg(feature = "test-support")]
    publication_pause: Option<(
        workspace_ids::WorkspaceId,
        collaboration_central_core::PausePoints,
    )>,
    #[cfg(feature = "test-support")]
    batch_waiting: watch::Sender<usize>,
    #[cfg(feature = "test-support")]
    central_publication_pauses: collaboration_central_core::PausePoints,
}

impl ObjectStorage {
    #[must_use]
    pub fn new(path: PathBuf, migrator: Arc<Migrator>) -> Self {
        Self {
            path,
            migrator,
            store: OnceCell::new(),
            batch_order: Mutex::new(()),
            #[cfg(feature = "test-support")]
            alarm_batch_failure: Default::default(),
            alarm: watch::Sender::new(None),
            #[cfg(feature = "test-support")]
            publication_pause: None,
            #[cfg(feature = "test-support")]
            batch_waiting: watch::Sender::new(0),
            #[cfg(feature = "test-support")]
            central_publication_pauses: Default::default(),
        }
    }

    async fn opened(&self) -> Result<&SqliteObjectStore, StoreFailure> {
        self.store
            .get_or_try_init(|| async {
                SqliteObjectStore::open_and_migrate(&self.path, &self.migrator)
                    .await
                    .map_err(|failure| {
                        tracing::error!(path = %self.path.display(), error = %failure, "object store failed to open");
                        StoreFailure::Unavailable
                    })
            })
            .await
    }

    pub async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        let _order = self.lock_batch_order().await;
        #[cfg(feature = "test-support")]
        if batch.statements.is_empty()
            && matches!(batch.alarm, central_storage::AlarmUpdate::SetFromQuery(_))
            && let Some(failure) = self
                .alarm_batch_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        {
            return Err(failure);
        }
        let (outcome, alarm) = self.opened().await?.run_batch(batch).await?;
        #[cfg(feature = "test-support")]
        if matches!(outcome, BatchOutcome::Applied { .. }) {
            self.central_publication_pauses
                .reached_central(collaboration_central_core::PausePoint::AfterStoreApplied)
                .await;
        }
        #[cfg(feature = "test-support")]
        if matches!(outcome, BatchOutcome::Applied { .. })
            && let Some((workspace, pauses)) = &self.publication_pause
        {
            pauses
                .reached(
                    workspace,
                    collaboration_central_core::PausePoint::AfterStoreApplied,
                )
                .await;
        }
        match alarm {
            AlarmChange::Unchanged => {}
            AlarmChange::Set(at) => {
                self.alarm.send_replace(Some(at));
            }
            AlarmChange::Clear => {
                self.alarm.send_replace(None);
            }
        }
        Ok(outcome)
    }

    async fn lock_batch_order(&self) -> MutexGuard<'_, ()> {
        #[cfg(not(feature = "test-support"))]
        {
            self.batch_order.lock().await
        }
        #[cfg(feature = "test-support")]
        {
            use std::future::Future;
            let mut acquisition = std::pin::pin!(self.batch_order.lock());
            let mut waiting = None;
            std::future::poll_fn(|context| match acquisition.as_mut().poll(context) {
                std::task::Poll::Pending => {
                    // Report only a genuinely pending acquisition, never an uncontended try.
                    waiting.get_or_insert_with(|| BatchWaiter::enter(self.batch_waiting.clone()));
                    std::task::Poll::Pending
                }
                ready @ std::task::Poll::Ready(_) => ready,
            })
            .await
        }
    }

    pub async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        self.opened().await?.read(statement).await
    }

    #[must_use]
    pub fn subscribe_alarm(&self) -> watch::Receiver<Option<SyncTime>> {
        self.alarm.subscribe()
    }

    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn hold_alarm_publication(&self) -> collaboration_central_core::HeldPause {
        self.central_publication_pauses
            .hold_central(collaboration_central_core::PausePoint::AfterStoreApplied)
    }

    #[cfg(feature = "test-support")]
    #[must_use]
    /// Uses the existing pause seam after commit and before publishing the retained alarm.
    pub fn with_publication_pause(
        mut self,
        workspace: workspace_ids::WorkspaceId,
        pauses: collaboration_central_core::PausePoints,
    ) -> Self {
        self.publication_pause = Some((workspace, pauses));
        self
    }

    #[cfg(feature = "test-support")]
    /// Resolves only after another batch's ordering-lock acquisition has returned Pending.
    pub async fn until_batch_waiting(&self) {
        let mut waiting = self.batch_waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting > 0).await;
    }

    /// The alarm time the last committed `SetFromQuery` set, if any. The Timers slice
    /// drives the alarm from this value; until then only tests read it.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn alarm(&self) -> Option<SyncTime> {
        *self.alarm.borrow()
    }

    #[cfg(feature = "test-support")]
    pub fn fail_next_alarm_batch(&self, failure: StoreFailure) {
        *self
            .alarm_batch_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure);
    }

    pub async fn close(&self) {
        if let Some(store) = self.store.get() {
            store.close().await;
        }
    }
}

#[cfg(feature = "test-support")]
struct BatchWaiter(watch::Sender<usize>);

#[cfg(feature = "test-support")]
impl BatchWaiter {
    fn enter(waiting: watch::Sender<usize>) -> Self {
        waiting.send_modify(|count| *count = count.saturating_add(1));
        Self(waiting)
    }
}

#[cfg(feature = "test-support")]
impl Drop for BatchWaiter {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count = count.saturating_sub(1));
    }
}
