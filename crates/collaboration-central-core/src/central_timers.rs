//! Organization gating and startup re-publication; the engine owns due work.
use crate::WorkspaceRuntime;
use central_storage::{Statement, StoreFailure};
use fleet_contracts::OrganizationStamp;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn alarm_organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        self.sign_in().organization_is_provisioned().await
    }
    pub async fn recompute_alarm(&self) -> Result<(), StoreFailure> {
        let query = if self.alarm_organization_is_provisioned().await? {
            central_engine_model::next_due_sql(crate::engine::query_registry::ALARM_INPUTS)
        } else {
            Statement {
                sql: "SELECT NULL,NULL",
                args: vec![],
            }
        };
        crate::engine::port::Engine::new(self)
            .publish_engine_query(query)
            .await
            .map(|_| ())
            .map_err(|error| error.failure)
    }
}
