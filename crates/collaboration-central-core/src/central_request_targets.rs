//! Central recipient lookup for admitted Send/Set and notice requests.
use crate::WorkspaceRuntime;
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::{
    CentralStore, WorkspaceStores, guarded_statements::text, workspace_decode_error::corrupt,
};
use workspace_ids::SessionRef;
use workspace_model::ActorIdentityId;

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
    pub(crate) async fn route_notice_actor(
        &self,
        actor: &ActorIdentityId,
    ) -> Result<Option<SessionRef>, StoreFailure> {
        let actors = self
            .access()
            .actor_views(std::slice::from_ref(actor))
            .await?;
        match actors.first().map(|actor| &actor.identity) {
            Some(workspace_model::Identity::AgentSession { session }) => Ok(Some(session.clone())),
            Some(
                workspace_model::Identity::Person { .. }
                | workspace_model::Identity::RouterComponent { .. },
            ) => Ok(None),
            None => Err(corrupt("notice recipient")),
        }
    }
}
