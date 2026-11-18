//! Native composition of org cores over the statement-only SQLite capability.
use crate::{object_storage::ObjectStorage, sign_in_trust::NativeSignInTrust};
#[path = "workspace_alarm_owner.rs"]
mod workspace_alarm_owner;
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::{
    OrganizationAdminAnswer, OrganizationSignInPort, SignInTrustSettings, WorkspaceCaller,
    WorkspaceRuntime,
    identity_contract::{
        AuthorizationRevision, AuthorizationRevisionAcknowledgement,
        OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_authority::OrganizationProvisioningFailure,
    person_request::PersonRequestInput,
};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime::{ClockIds, FleetRuntime};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_runtime_ports::{Clock, IdSource, SyncTime};
use router_clock::RuntimeClock;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use workspace_alarm_owner::WorkspaceAlarmOwner;
use workspace_model::central_mail::{CentralMailRequest, CentralMailResult};
use workspace_model::{WorkspaceId, WorkspaceReply, WorkspaceRequest};

impl CentralStore for ObjectStorage {
    async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        ObjectStorage::run_batch(self, batch).await
    }
    async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        ObjectStorage::read(self, statement).await
    }
    async fn published_alarm(&self) -> Result<Option<chrono::DateTime<chrono::Utc>>, StoreFailure> {
        ObjectStorage::published_alarm(self)
    }
}
#[derive(Clone)]
struct ClockHandle(Arc<dyn RuntimeClock>);
impl Clock for ClockHandle {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(runtime_primitives::Clock::now(self.0.as_ref()).unix_millis())
    }
}

#[cfg(test)]
#[path = "workspace_host_test.rs"]
mod tests;
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
impl WorkspaceStores for TokioWorkspaceStores {
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
    sign_in: Option<Arc<NativeSignInTrust>>,
    stores: TokioWorkspaceStores,
    orgs:
        Mutex<BTreeMap<OrganizationId, Arc<WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>>>>,
    alarms: Mutex<Vec<WorkspaceAlarmOwner>>,
}
impl TokioWorkspaceHost {
    pub async fn open(root: &Path, clock: Arc<dyn RuntimeClock>) -> Result<Self, StoreFailure> {
        Self::open_configured(root, clock, None).await
    }

    /// Sign-in trust is supplied before reopening any on-disk organization.
    pub async fn open_with_signin(
        root: &Path,
        clock: Arc<dyn RuntimeClock>,
        settings: SignInTrustSettings,
    ) -> Result<Self, StoreFailure> {
        let trust = NativeSignInTrust::new(settings).map_err(|_| StoreFailure::Unavailable)?;
        Self::open_configured(root, clock, Some(Arc::new(trust))).await
    }

    async fn open_configured(
        root: &Path,
        clock: Arc<dyn RuntimeClock>,
        sign_in: Option<Arc<NativeSignInTrust>>,
    ) -> Result<Self, StoreFailure> {
        tokio::fs::create_dir_all(root.join("central"))
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        let host = Self {
            root: root.to_owned(),
            clock,
            sign_in,
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
            // Organizations open independently: one bad store never blocks the rest.
            if let Err(error) = host.central(&organization).await {
                tracing::warn!(?error, organization = %organization.as_str(), "central organization open failed; continuing other organizations");
            }
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
        let clock_handle = ClockHandle(self.clock.clone());
        let ids: Arc<dyn IdSource> = Arc::new(ClockIds::new(clock_handle.clone()));
        let clock: Arc<dyn Clock> = Arc::new(clock_handle);
        let due_batches = Arc::new(fleet_runtime::DueBatchState::default());
        let storage = Arc::new(
            ObjectStorage::new(
                self.root
                    .join("central")
                    .join(format!("{}.sqlite", organization.as_str())),
                Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
            )
            .with_engine_publication(
                clock.clone(),
                fleet_runtime::engine_alarm_decider(due_batches.clone()),
            ),
        );
        let runtime = FleetRuntime::open(
            organization.clone(),
            storage,
            self.stores.clone(),
            clock.clone(),
            ids,
            due_batches,
            Arc::new(crate::native_logging::observe),
        );
        let verification = self
            .sign_in
            .as_ref()
            .map(|trust| trust.for_organization(organization.clone(), clock))
            .transpose();
        let runtime = Arc::new(WorkspaceRuntime::from_runtime(runtime, verification)?);
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
    /// Reads an already opened fixture organization without creating a store.
    #[cfg(feature = "test-support")]
    pub async fn observe_signin_organization(
        &self,
        organization: &OrganizationId,
    ) -> Result<collaboration_central_core::signin_test_observation::SignInObservation, StoreFailure>
    {
        use collaboration_central_core::signin_test_observation::{
            SignInObservation, observe_signin_organization,
        };
        let runtime = self.orgs.lock().await.get(organization).cloned();
        match runtime {
            Some(runtime) => {
                observe_signin_organization(runtime.statement_host(), organization).await
            }
            None => Ok(SignInObservation::nothing_stored()),
        }
    }

    #[cfg(feature = "test-support")]
    /// Lists already opened fixture organizations without opening another store.
    pub async fn signin_live_organizations(&self) -> Vec<OrganizationId> {
        self.orgs.lock().await.keys().cloned().collect()
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

impl OrganizationSignInPort for TokioWorkspaceHost {
    async fn provision(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        self.central(&request.organization_id)
            .await
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .provision(request)
            .await
    }

    async fn digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        self.central(&request.organization_id)
            .await
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .digest(request)
            .await
    }

    async fn revision(
        &self,
        request: &AuthorizationRevision,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        self.central(request.organization_id())
            .await
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .revision(request)
            .await
    }

    async fn admin(
        &self,
        organization: OrganizationId,
        input: PersonRequestInput,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        self.central(&organization)
            .await?
            .admin(organization, input)
            .await
    }
    async fn device_login(
        &self,
        request: &collaboration_central_core::identity_contract::DecideDeviceLogin,
    ) -> Result<collaboration_central_core::identity_contract::DeviceLoginDecision, StoreFailure>
    {
        self.central(&request.organization_id)
            .await?
            .device_login(request)
            .await
    }
    async fn machine_token(
        &self,
        request: &collaboration_central_core::identity_contract::DecideMachineToken,
    ) -> Result<collaboration_central_core::identity_contract::MachineTokenDecision, StoreFailure>
    {
        self.central(&request.organization_id)
            .await?
            .machine_token(request)
            .await
    }
}
