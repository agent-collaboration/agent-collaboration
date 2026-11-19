//! The client's prompt cancellation: it cancels a session's turn through
//! the connection, and propagates the Router's turn cancellation to the
//! provider's pending permissions through the interaction port.

use super::{AgentSessionClient, ExternalProviderRuntimeError, ProviderCommand};
use crate::InteractionPort;
use crate::approval_turn_cancellation::active_turn_cancellation;

impl<P: InteractionPort> AgentSessionClient<P> {
    pub async fn cancel_active_prompt(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.cancel_prompt(provider_session_id, None).await
    }

    pub async fn cancel_prompt_operation(
        &self,
        provider_session_id: String,
        expected_operation_id: P::OperationId,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.cancel_prompt(provider_session_id, Some(expected_operation_id))
            .await
    }

    pub(crate) async fn cancel_prompt(
        &self,
        provider_session_id: String,
        expected_operation_id: Option<P::OperationId>,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let turn_cancellation = active_turn_cancellation(
            &self.approval_contexts,
            &self.interaction_port,
            &provider_session_id,
            expected_operation_id.as_ref(),
        );
        if let Some(turn_cancellation) = &turn_cancellation {
            turn_cancellation.mark_cancelling();
            turn_cancellation.settle_pending_approvals().await;
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::Cancel {
                provider_session_id,
                expected_operation_id,
                reply,
            })
            .await
            .map_err(|_| self.cancel_transport_failure())?;
        result
            .await
            .map_err(|_| self.cancel_transport_failure())??;
        Ok(())
    }
}

impl<P: InteractionPort> AgentSessionClient<P> {
    fn cancel_transport_failure(&self) -> ExternalProviderRuntimeError {
        if self.sink_closed.is_cancelled() {
            ExternalProviderRuntimeError::SinkClosed
        } else {
            ExternalProviderRuntimeError::Operation("provider runtime closed".to_owned())
        }
    }
}
