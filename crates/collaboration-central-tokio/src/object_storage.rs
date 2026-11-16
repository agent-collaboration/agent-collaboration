//! One object's storage on the Tokio host: its SQLite file, opened and migrated on
//! first use, and the alarm time its last committed batch asked for.

use crate::sqlite_object_store::{AlarmChange, SqliteObjectStore};
use central_storage::{
    AlarmPublication, AlarmPublicationFailure, BatchOutcome, EngineAlarmDecider, EngineAlarmTimes,
    ReadOutcome, Statement, StatementBatch, StoreFailure,
};
use runtime_primitives::{Clock, SyncTime};
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

struct EnginePublication {
    clock: Arc<dyn Clock>,
    decide: EngineAlarmDecider,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LastAlarmPublication {
    Published { deadline: Option<SyncTime> },
    Failed { at: SyncTime },
}

pub struct ObjectStorage {
    #[cfg(feature = "test-support")]
    engine_commit_fault: std::sync::Mutex<Option<collaboration_central_core::EngineCommitFault>>,
    #[cfg(feature = "test-support")]
    engine_read_fault: std::sync::Mutex<Option<collaboration_central_core::EngineReadFault>>,
    path: PathBuf,
    migrator: Arc<Migrator>,
    store: OnceCell<SqliteObjectStore>,
    // A database commit and its retained alarm are one ordered object write.
    batch_order: Mutex<()>,
    #[cfg(feature = "test-support")]
    alarm_batch_failure: std::sync::Mutex<Option<StoreFailure>>,
    /// Kept in memory after commit (Program Design, "the one alarm"; Tokio).
    alarm: watch::Sender<Option<SyncTime>>,
    publication_status: watch::Sender<LastAlarmPublication>,
    #[cfg(feature = "test-support")]
    publication_pause: Option<(
        workspace_ids::WorkspaceId,
        collaboration_central_core::PausePoints,
    )>,
    #[cfg(feature = "test-support")]
    batch_waiting: watch::Sender<usize>,
    #[cfg(feature = "test-support")]
    central_publication_pauses: collaboration_central_core::PausePoints,
    engine_publication: Option<EnginePublication>,
    #[cfg(feature = "test-support")]
    publication_failures: std::sync::Mutex<Vec<AlarmPublicationFailure>>,
}

impl ObjectStorage {
    #[must_use]
    pub fn new(path: PathBuf, migrator: Arc<Migrator>) -> Self {
        Self {
            #[cfg(feature = "test-support")]
            engine_commit_fault: Default::default(),
            #[cfg(feature = "test-support")]
            engine_read_fault: Default::default(),
            path,
            migrator,
            store: OnceCell::new(),
            batch_order: Mutex::new(()),
            #[cfg(feature = "test-support")]
            alarm_batch_failure: Default::default(),
            alarm: watch::Sender::new(None),
            publication_status: watch::Sender::new(LastAlarmPublication::Published {
                deadline: None,
            }),
            #[cfg(feature = "test-support")]
            publication_pause: None,
            #[cfg(feature = "test-support")]
            batch_waiting: watch::Sender::new(0),
            #[cfg(feature = "test-support")]
            central_publication_pauses: Default::default(),
            engine_publication: None,
            #[cfg(feature = "test-support")]
            publication_failures: Default::default(),
        }
    }

    /// Modern central shares this clock with its runtime and identifier source.
    pub fn with_engine_publication(
        mut self,
        clock: Arc<dyn Clock>,
        decide: EngineAlarmDecider,
    ) -> Self {
        self.engine_publication = Some(EnginePublication { clock, decide });
        self
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
        let publishes = !matches!(batch.alarm, central_storage::AlarmUpdate::Unchanged);
        let result = self.run_ordered_batch(batch).await;
        if publishes && let Some(engine) = &self.engine_publication {
            let status = match &result {
                Ok(BatchOutcome::Applied {
                    publication: AlarmPublication::Set { .. } | AlarmPublication::Cleared,
                    ..
                }) => Some(LastAlarmPublication::Published {
                    deadline: *self.alarm.borrow(),
                }),
                Ok(BatchOutcome::Applied {
                    publication: AlarmPublication::Failed { .. },
                    ..
                })
                | Err(_) => Some(LastAlarmPublication::Failed {
                    at: engine.clock.now(),
                }),
                Ok(
                    BatchOutcome::Applied {
                        publication: AlarmPublication::Unchanged,
                        ..
                    }
                    | BatchOutcome::GuardFailed { .. }
                    | BatchOutcome::ConstraintViolated { .. }
                    | BatchOutcome::StatementRejected { .. },
                ) => None,
            };
            if let Some(status) = status {
                self.publication_status.send_replace(status);
            }
        }
        result
    }

    async fn run_ordered_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        #[cfg(feature = "test-support")]
        let commit_fault = {
            let mut armed = self
                .engine_commit_fault
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if armed.as_ref().is_some_and(|fault| fault.matches(&batch)) {
                armed.take()
            } else {
                None
            }
        };
        #[cfg(feature = "test-support")]
        if let Some(result) = commit_fault.as_ref().and_then(|fault| fault.before()) {
            return result;
        }
        if matches!(
            &batch.alarm,
            central_storage::AlarmUpdate::SetEngineFromQuery { .. }
        ) && self.engine_publication.is_none()
        {
            return Err(StoreFailure::AlarmUnpublishable {
                reason: AlarmPublicationFailure::ClockUnavailable,
            });
        }
        #[cfg(feature = "test-support")]
        if batch.statements.is_empty()
            && matches!(
                batch.alarm,
                central_storage::AlarmUpdate::SetFromQuery(_)
                    | central_storage::AlarmUpdate::SetEngineFromQuery { .. }
            )
            && let Some(failure) = self
                .alarm_batch_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        {
            return Err(failure);
        }
        let (mut outcome, alarm) = self.opened().await?.run_batch(batch).await?;
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
        let publication = match alarm {
            AlarmChange::Unchanged => AlarmPublication::Unchanged,
            AlarmChange::Set(at) => {
                self.alarm.send_replace(Some(at));
                AlarmPublication::Set {
                    unix_millis: at.unix_millis(),
                }
            }
            AlarmChange::Clear => {
                self.alarm.send_replace(None);
                AlarmPublication::Cleared
            }
            AlarmChange::Engine {
                earliest_on_grid_due,
                earliest_legacy_due,
            } => self.publish_engine_alarm(earliest_on_grid_due, earliest_legacy_due),
        };
        if let BatchOutcome::Applied {
            publication: receipt,
            ..
        } = &mut outcome
        {
            #[cfg(feature = "test-support")]
            if let AlarmPublication::Failed { reason } = &publication {
                self.publication_failures
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(*reason);
            }
            *receipt = publication;
        }
        #[cfg(feature = "test-support")]
        if commit_fault.is_some_and(|fault| {
            matches!(
                fault.kind,
                collaboration_central_core::EngineCommitFaultKind::AppliedAnswerLost
            )
        }) && matches!(outcome, BatchOutcome::Applied { .. })
        {
            return Err(StoreFailure::AnswerLost);
        }
        Ok(outcome)
    }

    fn publish_engine_alarm(&self, grid: Option<i64>, legacy: Option<i64>) -> AlarmPublication {
        let decide = || -> Result<Option<i64>, AlarmPublicationFailure> {
            let publication = self
                .engine_publication
                .as_ref()
                .ok_or(AlarmPublicationFailure::ClockUnavailable)?;
            let mut write = |time: Option<i64>| {
                self.alarm
                    .send_replace(time.map(SyncTime::from_unix_millis));
            };
            (publication.decide)(
                EngineAlarmTimes {
                    earliest_on_grid_due_unix_millis: grid,
                    earliest_legacy_due_unix_millis: legacy,
                },
                publication.clock.now().unix_millis(),
                &mut write,
            )
        };
        match decide() {
            Ok(None) => AlarmPublication::Cleared,
            Ok(Some(unix_millis)) => AlarmPublication::Set { unix_millis },
            Err(reason) => AlarmPublication::Failed { reason },
        }
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
        #[cfg(feature = "test-support")]
        {
            let mut fault = self
                .engine_read_fault
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if fault
                .as_ref()
                .is_some_and(|fault| fault.matches(&statement))
            {
                fault.take();
                return Err(StoreFailure::Corrupt {
                    reason: central_storage::CorruptReason::BadValue {
                        column: "engine.due_list".into(),
                    },
                });
            }
        }
        self.opened().await?.read(statement).await
    }

    #[must_use]
    pub fn subscribe_alarm(&self) -> watch::Receiver<Option<SyncTime>> {
        self.alarm.subscribe()
    }

    /// The last successfully published physical deadline; a failed publication
    /// keeps the previous one, so health reports what is actually armed.
    pub fn published_alarm(&self) -> Result<Option<chrono::DateTime<chrono::Utc>>, StoreFailure> {
        (*self.alarm.borrow())
            .map(|deadline| {
                chrono::DateTime::from_timestamp_millis(deadline.unix_millis()).ok_or_else(|| {
                    StoreFailure::Corrupt {
                        reason: central_storage::CorruptReason::BadValue {
                            column: "alarm.deadline".into(),
                        },
                    }
                })
            })
            .transpose()
    }

    pub(crate) fn subscribe_publication_status(&self) -> watch::Receiver<LastAlarmPublication> {
        self.publication_status.subscribe()
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
    #[cfg(feature = "test-support")]
    pub fn inject_engine_commit_fault(&self, fault: collaboration_central_core::EngineCommitFault) {
        *self
            .engine_commit_fault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fault);
    }
    #[cfg(feature = "test-support")]
    pub fn inject_engine_read_fault(&self, fault: collaboration_central_core::EngineReadFault) {
        *self
            .engine_read_fault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(fault);
    }

    #[cfg(feature = "test-support")]
    pub fn take_alarm_publication_failures(&self) -> Vec<AlarmPublicationFailure> {
        std::mem::take(
            &mut *self
                .publication_failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    #[cfg(feature = "test-support")]
    pub fn failed_alarm_publication_at(&self) -> Option<SyncTime> {
        match *self.publication_status.borrow() {
            LastAlarmPublication::Published { .. } => None,
            LastAlarmPublication::Failed { at } => Some(at),
        }
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
