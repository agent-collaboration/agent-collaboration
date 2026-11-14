//! Native composition of org cores over the statement-only SQLite capability.
use crate::object_storage::ObjectStorage;
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::{
    ClockIds, ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls,
};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use workspace_model::{WorkspaceId, WorkspaceReply, WorkspaceRequest};

impl ObjectHost for ObjectStorage {
    async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        ObjectStorage::run_batch(self, batch).await
    }
    async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        ObjectStorage::read(self, statement).await
    }
}
#[derive(Clone)]
struct ClockHandle(Arc<dyn Clock>);
impl Clock for ClockHandle {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}
#[derive(Clone)]
pub struct TokioWorkspaceStores {
    root: PathBuf,
    stores: Arc<Mutex<BTreeMap<WorkspaceId, Arc<ObjectStorage>>>>,
}
impl TokioWorkspaceStores {
    async fn store(&self, id: &WorkspaceId) -> Result<Arc<ObjectStorage>, StoreFailure> {
        let mut stores = self.stores.lock().await;
        if let Some(store) = stores.get(id) {
            return Ok(store.clone());
        }
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        let store = Arc::new(ObjectStorage::new(
            self.root.join(format!("{id}.sqlite")),
            Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
        ));
        stores.insert(id.clone(), store.clone());
        Ok(store)
    }
}
impl WorkspaceStoreCalls for TokioWorkspaceStores {
    async fn call_workspace(
        &self,
        workspace: &WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        let store = self.store(workspace).await?;
        match call {
            StoreCall::Read(statement) => store.read(statement).await.map(StoreReply::Read),
            StoreCall::Batch(mut batch) => {
                batch.alarm = central_storage::AlarmUpdate::Unchanged;
                store.run_batch(batch).await.map(StoreReply::Batch)
            }
        }
    }
}
/// One process-local owner of each org, with workspace stores shared by their UUID.
pub struct TokioWorkspaceHost {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    stores: TokioWorkspaceStores,
    orgs:
        Mutex<BTreeMap<OrganizationId, Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>>>,
}
impl TokioWorkspaceHost {
    pub async fn open(root: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreFailure> {
        tokio::fs::create_dir_all(root.join("central"))
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        Ok(Self {
            root: root.to_owned(),
            clock,
            stores: TokioWorkspaceStores {
                root: root.join("workspace-stores"),
                stores: Default::default(),
            },
            orgs: Default::default(),
        })
    }
    pub async fn central(
        &self,
        organization: &OrganizationId,
    ) -> Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>> {
        let mut orgs = self.orgs.lock().await;
        orgs.entry(organization.clone())
            .or_insert_with(|| {
                let storage = Arc::new(ObjectStorage::new(
                    self.root
                        .join("central")
                        .join(format!("{}.sqlite", organization.as_str())),
                    Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
                ));
                let ids: Arc<dyn IdSource> =
                    Arc::new(ClockIds::new(ClockHandle(self.clock.clone())));
                Arc::new(WorkspaceRuntime::new(
                    organization.clone(),
                    storage,
                    self.stores.clone(),
                    self.clock.clone(),
                    ids,
                ))
            })
            .clone()
    }
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: WorkspaceRequest,
    ) -> Result<WorkspaceReply, StoreFailure> {
        self.central(&caller.organization_id)
            .await
            .request(caller, request)
            .await
    }
    pub async fn close(&self) {
        for runtime in self.orgs.lock().await.values() {
            runtime.statement_host().close().await;
        }
        for store in self.stores.stores.lock().await.values() {
            store.close().await;
        }
    }
}
