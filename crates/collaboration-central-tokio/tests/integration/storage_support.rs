//! Shared helpers: a held clock, the workspace store calls central makes over a
//! temporary root, and row readers.

use central_storage::{
    AlarmUpdate, ReadOutcome, SqlValue, Statement, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_tokio::ObjectStorage;
use fleet_runtime::ClockIds;
use fleet_runtime_ports::{Clock, IdSource, SyncTime};
use sqlx::migrate::Migrator;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::Mutex;
use workspace_ids::WorkspaceId;

/// The one injected clock: it moves only when a case moves it.
pub struct HeldClock(AtomicI64);
impl HeldClock {
    pub fn at(milliseconds: i64) -> Arc<Self> {
        Arc::new(Self(AtomicI64::new(milliseconds)))
    }
    pub fn set(&self, milliseconds: i64) {
        self.0.store(milliseconds, Ordering::SeqCst);
    }
}
impl Clock for HeldClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
struct SharedHeldClock(Arc<HeldClock>);
impl Clock for SharedHeldClock {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::try_from("019f0000-0000-7000-8000-0000000000aa".to_owned()).unwrap()
}

/// Central's calls into workspace stores, made exactly as the host's
/// `TokioWorkspaceStores` makes them (one `ObjectStorage` per workspace, a
/// batch's alarm update ignored), but with the case's own workspace migrator.
#[derive(Clone)]
pub struct WorkspaceStorePlatform {
    root: PathBuf,
    migrator: Arc<Migrator>,
    ids: Arc<ClockIds<SharedHeldClock>>,
    stores: Arc<Mutex<BTreeMap<WorkspaceId, Arc<ObjectStorage>>>>,
}
impl WorkspaceStorePlatform {
    async fn store(&self, workspace: &WorkspaceId) -> Arc<ObjectStorage> {
        let mut stores = self.stores.lock().await;
        stores
            .entry(workspace.clone())
            .or_insert_with(|| {
                Arc::new(ObjectStorage::new(
                    self.root.join(format!("{workspace}.sqlite")),
                    self.migrator.clone(),
                ))
            })
            .clone()
    }
    pub async fn call_store(
        &self,
        workspace: WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        let store = self.store(&workspace).await;
        match call {
            StoreCall::Read(statement) => store.read(statement).await.map(StoreReply::Read),
            StoreCall::Batch(mut batch) => {
                batch.alarm = AlarmUpdate::Unchanged;
                store.run_batch(batch).await.map(StoreReply::Batch)
            }
        }
    }
    pub fn ids(&self) -> &dyn IdSource {
        self.ids.as_ref()
    }
    pub async fn shutdown(&self) -> Result<(), StoreFailure> {
        for store in std::mem::take(&mut *self.stores.lock().await).into_values() {
            store.close().await;
        }
        Ok(())
    }
}

/// Workspace stores run `workspace_migrator`.
pub async fn open_platform(
    root: &Path,
    clock: Arc<HeldClock>,
    workspace_migrator: Migrator,
) -> WorkspaceStorePlatform {
    tokio::fs::create_dir_all(root).await.unwrap();
    WorkspaceStorePlatform {
        root: root.to_path_buf(),
        migrator: Arc::new(workspace_migrator),
        ids: Arc::new(ClockIds::new(SharedHeldClock(clock))),
        stores: Arc::default(),
    }
}

pub async fn read_store(platform: &WorkspaceStorePlatform, statement: Statement) -> ReadOutcome {
    match platform
        .call_store(workspace(), StoreCall::Read(statement))
        .await
        .unwrap()
    {
        StoreReply::Read(outcome) => outcome,
        StoreReply::Batch(outcome) => panic!("a read answered a batch: {outcome:?}"),
    }
}

pub fn single_integer(outcome: &ReadOutcome) -> i64 {
    match outcome.rows.as_slice() {
        [row] => match row.as_slice() {
            [SqlValue::Integer(value)] => value.get(),
            other => panic!("expected one integer, got {other:?}"),
        },
        other => panic!("expected one row, got {other:?}"),
    }
}

pub fn texts_in_column(outcome: &ReadOutcome, column: usize) -> Vec<String> {
    outcome
        .rows
        .iter()
        .map(|row| match row.get(column) {
            Some(SqlValue::Text(value)) => value.clone(),
            other => panic!("expected text, got {other:?}"),
        })
        .collect()
}
