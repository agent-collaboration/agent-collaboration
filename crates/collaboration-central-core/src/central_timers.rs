//! Organization gating and startup re-publication; the engine owns due work.
use crate::WorkspaceRuntime;
use central_storage::StoreFailure;
use fleet_contracts::OrganizationStamp;
use fleet_engine::Engine;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn alarm_organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        self.sign_in().organization_is_provisioned().await
    }
    pub async fn recompute_alarm(&self) -> Result<(), StoreFailure> {
        Engine::new(self)
            .recompute_alarm(self.alarm_organization_is_provisioned().await?)
            .await
    }
}
