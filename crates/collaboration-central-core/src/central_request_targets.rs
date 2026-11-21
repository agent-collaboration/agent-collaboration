//! Central recipient lookup for admitted Send/Set and notice requests.
use crate::WorkspaceRuntime;
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::{
    CentralStore, WorkspaceStores, guarded_statements::text, workspace_decode_error::corrupt,
};
use workspace_ids::SessionRef;

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn known_session(&self, target: &SessionRef) -> Result<bool, StoreFailure> {
        let identity =
            serde_json_canonicalizer::to_string(&workspace_model::Identity::AgentSession {
                session: target.clone(),
            })
            .map_err(|_| corrupt("session identity"))?;
        let rows=self.execution.statement_host().read(Statement{sql:"SELECT a.actor_identity_id FROM access_actor_identities a JOIN login_machines m ON m.machine_id=? WHERE a.identity_kind='agentSession' AND a.identity=? AND m.state='enrolled'",args:vec![text(&target.machine_id),text(identity)]}).await?;
        Ok(!rows.rows.is_empty())
    }
}
