//! Organization gating and startup re-publication; the engine owns due work.
use crate::WorkspaceRuntime;
use central_storage::StoreFailure;
use fleet_engine::Engine;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn alarm_organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        match crate::prepare_central_alarm(self.statement_host()).await? {
            crate::CentralOrganizationIdentity::Unprovisioned => Ok(false),
            crate::CentralOrganizationIdentity::Stamped { organization_id }
                if organization_id == *self.fleet_runtime.organization() =>
            {
                Ok(true)
            }
            crate::CentralOrganizationIdentity::Stamped { .. } => {
                Err(corrupt("organization_identity.id"))
            }
        }
    }
    pub async fn recompute_alarm(&self) -> Result<(), StoreFailure> {
        Engine::new(self)
            .recompute_alarm(self.alarm_organization_is_provisioned().await?)
            .await
    }
}
