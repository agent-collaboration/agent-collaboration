//! The target admission plan is the only source of a foreign native creator route.
use super::approval_route_storage::stored_route_is_eligible;
use super::{ApprovalBrokerError, ApprovalRoute, ServiceInteractionBroker};
use crate::{PreparedRemoteCreate, PreparedRemoteCreateActors};

impl ServiceInteractionBroker {
    pub async fn register_remote_create_route(
        &self,
        prepared: &PreparedRemoteCreate,
        mut route: ApprovalRoute,
    ) -> Result<(), ApprovalBrokerError> {
        let PreparedRemoteCreateActors::Codex {
            creator, approver, ..
        } = prepared.actors()
        else {
            return Err(ApprovalBrokerError::RouteUnavailable);
        };
        if prepared.entry().request.endpoint != self.backend.endpoint
            || route.created_by != *creator
            || route.approver != *approver
            || route.approver.endpoint.service_id != self.service_id
            || route.access != prepared.entry().request.requested_policy.access
            || route.source_installation.is_some()
            || route.thread_id.is_empty()
        {
            return Err(ApprovalBrokerError::RouteUnavailable);
        }
        route.source_installation = Some(prepared.entry().source_installation.clone());
        if !stored_route_is_eligible(&route, &self.service_id) {
            return Err(ApprovalBrokerError::RouteUnavailable);
        }
        self.routes
            .lock()
            .await
            .insert(route.thread_id.clone(), route);
        self.persist_routes().await
    }
}
