//! Native composition of org cores over the statement-only SQLite capability.
use crate::object_storage::ObjectStorage;
#[path = "workspace_alarm_owner.rs"]
mod workspace_alarm_owner;
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::{
    ClockIds, ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls,
};
use collaboration_protocol::central_mail::{CentralMailRequest, CentralMailResult, MailPolicy};
use fleet_identity_protocol::OrganizationId;
use router_clock::RuntimeClock;
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use workspace_alarm_owner::WorkspaceAlarmOwner;
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
struct ClockHandle(Arc<dyn RuntimeClock>);
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
    clock: Arc<dyn RuntimeClock>,
    mail_policy: MailPolicy,
    stores: TokioWorkspaceStores,
    orgs:
        Mutex<BTreeMap<OrganizationId, Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>>>,
    alarms: Mutex<Vec<WorkspaceAlarmOwner>>,
}
impl TokioWorkspaceHost {
    pub async fn open(root: &Path, clock: Arc<dyn RuntimeClock>) -> Result<Self, StoreFailure> {
        Self::open_with_mail_policy(root, clock, MailPolicy::default()).await
    }
    pub async fn open_with_mail_policy(
        root: &Path,
        clock: Arc<dyn RuntimeClock>,
        mail_policy: MailPolicy,
    ) -> Result<Self, StoreFailure> {
        tokio::fs::create_dir_all(root.join("central"))
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        let host = Self {
            root: root.to_owned(),
            clock,
            mail_policy,
            stores: TokioWorkspaceStores {
                root: root.join("workspace-stores"),
                stores: Default::default(),
            },
            orgs: Default::default(),
            alarms: Default::default(),
        };
        let mut entries = tokio::fs::read_dir(root.join("central"))
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| StoreFailure::Unavailable)?
        {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("sqlite") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or(StoreFailure::Unavailable)?;
            let organization =
                OrganizationId::try_from(name.to_owned()).map_err(|_| StoreFailure::Unavailable)?;
            host.central(&organization).await?;
        }
        Ok(host)
    }
    pub async fn central(
        &self,
        organization: &OrganizationId,
    ) -> Result<Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>, StoreFailure> {
        let mut orgs = self.orgs.lock().await;
        if let Some(runtime) = orgs.get(organization) {
            return Ok(runtime.clone());
        }
        let storage = Arc::new(ObjectStorage::new(
            self.root
                .join("central")
                .join(format!("{}.sqlite", organization.as_str())),
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        ));
        let ids: Arc<dyn IdSource> = Arc::new(ClockIds::new(ClockHandle(self.clock.clone())));
        let runtime = Arc::new(
            WorkspaceRuntime::new(
                organization.clone(),
                storage,
                self.stores.clone(),
                Arc::new(ClockHandle(self.clock.clone())),
                ids,
            )
            .with_mail_policy(self.mail_policy),
        );
        runtime.recompute_alarm().await?;
        self.alarms.lock().await.push(WorkspaceAlarmOwner::start(
            runtime.clone(),
            self.clock.clone(),
        ));
        orgs.insert(organization.clone(), runtime.clone());
        Ok(runtime)
    }
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: WorkspaceRequest,
    ) -> Result<WorkspaceReply, StoreFailure> {
        self.central(&caller.organization_id)
            .await?
            .request(caller, request)
            .await
    }
    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: CentralMailRequest,
    ) -> Result<CentralMailResult, StoreFailure> {
        self.central(&caller.organization_id)
            .await?
            .request_central(caller, request)
            .await
    }
    pub async fn close(&self) {
        let alarms = std::mem::take(&mut *self.alarms.lock().await);
        for alarm in alarms {
            alarm.close().await;
        }
        for runtime in self.orgs.lock().await.values() {
            runtime.statement_host().close().await;
        }
        for store in self.stores.stores.lock().await.values() {
            store.close().await;
        }
    }
}
