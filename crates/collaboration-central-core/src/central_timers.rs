//! Organization gating and startup re-publication; the engine owns due work.
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls, workspace_decode_error::corrupt};
use central_storage::{Statement, StoreFailure};

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn alarm_organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        match crate::prepare_central_alarm(self.host.as_ref()).await? {
            crate::CentralOrganizationIdentity::Unprovisioned => Ok(false),
            crate::CentralOrganizationIdentity::Stamped { organization_id }
                if organization_id == self.organization =>
            {
                Ok(true)
            }
            crate::CentralOrganizationIdentity::Stamped { .. } => {
                Err(corrupt("signin_organization_identity.id"))
            }
        }
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
