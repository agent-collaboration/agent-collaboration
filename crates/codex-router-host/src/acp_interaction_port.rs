//! Managed ACP callback policy and its exact-turn cancellation port.

use acp_client_runtime::{InteractionFuture, InteractionPort};
use collaboration_protocol::OperationId;
use provider_session_machines::{SessionGeneration, session_turn_machine::TurnId};
use session_event_model::managed_interaction_refusal::ManagedInteractionRefusal;
use tokio_util::sync::CancellationToken;

use crate::{
    ExternalProviderApprovalContext, external_provider_approval_context::OwnerApprovalScope,
};
use std::sync::Arc;

pub(crate) trait RefusedTurnCancellation: Send + Sync {
    fn cancel_refused_turn(
        &self,
        session: String,
        scope: OwnerApprovalScope,
    ) -> InteractionFuture<'_, ()>;
}

pub(crate) struct HostInteractionPort(pub(crate) Arc<dyn RefusedTurnCancellation>);

impl InteractionPort for HostInteractionPort {
    type Context = ExternalProviderApprovalContext;
    type OperationId = OperationId;

    fn managed_refusal(&self) -> Option<ManagedInteractionRefusal> {
        Some(ManagedInteractionRefusal::ApprovalsUnavailableOnBuild)
    }

    fn cancel_after_managed_refusal(&self, context: Self::Context) -> InteractionFuture<'_, ()> {
        Box::pin(async move {
            if let Some(scope) = context.owner_scope {
                let session = String::from(context.target.session_id);
                self.0.cancel_refused_turn(session, scope).await;
            }
        })
    }

    fn operation_id(context: &Self::Context) -> Self::OperationId {
        context.operation_id.clone()
    }

    fn binding_retirement(context: &Self::Context) -> CancellationToken {
        context.binding_retirement.clone()
    }

    fn bind_owner_scope(context: &mut Self::Context, run: SessionGeneration, turn: TurnId) {
        context.owner_scope = Some(OwnerApprovalScope { run, turn });
    }
}
