//! One object's storage on the Tokio host: its SQLite file, opened and migrated on
//! first use, and the alarm time its last committed batch asked for.

use crate::sqlite_object_store::{AlarmChange, SqliteObjectStore};
use collaboration_central_core::storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreFailure,
};
use runtime_primitives::SyncTime;
use sqlx::migrate::Migrator;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{OnceCell, watch};

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
    /// Kept in memory after commit (Program Design, "the one alarm"; Tokio).
    alarm: watch::Sender<Option<SyncTime>>,
}

impl ObjectStorage {
    #[must_use]
    pub fn new(path: PathBuf, migrator: Arc<Migrator>) -> Self {
        Self {
            path,
            migrator,
            store: OnceCell::new(),
            alarm: watch::Sender::new(None),
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
        let (outcome, alarm) = self.opened().await?.run_batch(batch).await?;
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

    pub async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        self.opened().await?.read(statement).await
    }

    /// The alarm time the last committed `SetFromQuery` set, if any. The Timers slice
    /// drives the alarm from this value; until then only tests read it.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn alarm(&self) -> Option<SyncTime> {
        *self.alarm.borrow()
    }

    pub async fn close(&self) {
        if let Some(store) = self.store.get() {
            store.close().await;
        }
    }
}
