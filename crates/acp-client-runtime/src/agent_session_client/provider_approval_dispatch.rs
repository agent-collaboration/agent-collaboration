//! Approval context lifetime for a dispatched provider prompt.

use super::{
    ActiveApprovalContext, AgentSessionClient, ApprovalContextGuard, ExternalProviderRuntimeError,
};
use crate::{
    acp_transport::TransportRun,
    approval_turn_cancellation::{
        OwnerApprovalDrain, PermissionResponseBarrier, active_turn_cancellation,
    },
};
use provider_session_machines::session_owner::DispatchAuthorization;
use std::sync::Arc;

pub struct OwnerApprovalBindings<P: crate::InteractionPort> {
    active: Arc<std::sync::Mutex<std::collections::HashMap<String, ActiveApprovalContext<P>>>>,
    interaction_port: Arc<P>,
}

impl<P: crate::InteractionPort> Clone for OwnerApprovalBindings<P> {
    fn clone(&self) -> Self {
        Self {
            active: Arc::clone(&self.active),
            interaction_port: Arc::clone(&self.interaction_port),
        }
    }
}

pub struct OwnerApprovalLease<P: crate::InteractionPort> {
    _lease: ApprovalContextGuard<P>,
}

impl<P: crate::InteractionPort> OwnerApprovalBindings<P> {
    pub fn install(
        &self,
        provider_session_id: String,
        run: TransportRun,
        turn: provider_session_machines::session_turn_machine::TurnId,
        mut context: P::Context,
    ) -> Result<OwnerApprovalLease<P>, ExternalProviderRuntimeError> {
        let mut active = self.active.lock().map_err(|_| {
            ExternalProviderRuntimeError::Operation(
                "provider approval context unavailable".to_owned(),
            )
        })?;
        if active.contains_key(&provider_session_id) {
            return Err(ExternalProviderRuntimeError::Operation(
                "a provider approval context is already active".to_owned(),
            ));
        }
        let operation_id = P::operation_id(&context);
        P::bind_owner_scope(&mut context, run.generation, turn);
        active.insert(
            provider_session_id.clone(),
            ActiveApprovalContext {
                approval: context,
                generation: run.generation,
                incarnation: run.incarnation,
                turn: Some(turn),
                cancelling: tokio_util::sync::CancellationToken::new(),
                responses: Arc::new(PermissionResponseBarrier::default()),
            },
        );
        Ok(OwnerApprovalLease {
            _lease: ApprovalContextGuard {
                contexts: Arc::clone(&self.active),
                provider_session_id,
                operation_id,
            },
        })
    }

    pub fn begin_cancel(
        &self,
        provider_session_id: &str,
        authorization: &DispatchAuthorization,
    ) -> Option<OwnerApprovalDrain<P>> {
        let drain = active_turn_cancellation(
            &self.active,
            &self.interaction_port,
            provider_session_id,
            authorization,
        )?;
        drain.mark_cancelling();
        Some(drain)
    }
}

impl<P: crate::InteractionPort> AgentSessionClient<P> {
    #[must_use]
    pub fn owner_approval_bindings(&self) -> OwnerApprovalBindings<P> {
        OwnerApprovalBindings {
            active: Arc::clone(&self.approval_contexts),
            interaction_port: Arc::clone(&self.interaction_port),
        }
    }
}
