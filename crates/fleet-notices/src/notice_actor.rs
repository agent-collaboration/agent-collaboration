//! Resolve a notice recipient through the existing Access actor view.
use central_storage::StoreFailure;
use fleet_runtime_ports::{CentralStore, WorkspaceStores, workspace_decode_error::corrupt};
use fleet_workspace_access::WorkspaceAccess;
use workspace_ids::SessionRef;
use workspace_model::ActorIdentityId;

pub(crate) async fn route_notice_actor(
    access: &WorkspaceAccess<'_, impl CentralStore, impl WorkspaceStores>,
    actor: &ActorIdentityId,
) -> Result<Option<SessionRef>, StoreFailure> {
    let actors = access.actor_views(std::slice::from_ref(actor)).await?;
    match actors.first().map(|actor| &actor.identity) {
        Some(workspace_model::Identity::AgentSession { session }) => Ok(Some(session.clone())),
        Some(
            workspace_model::Identity::Person { .. }
            | workspace_model::Identity::RouterComponent { .. },
        ) => Ok(None),
        None => Err(corrupt("notice recipient")),
    }
}
