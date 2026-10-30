//! Typed prompt admission at the provider runtime boundary; preparation stays in its actor.

use super::{
    AgentSessionClient, ExternalProviderPromptOutcome, ExternalProviderRuntimeError,
    ProviderCommand, ProviderPromptDispatchObservation,
};
use crate::InteractionPort;
use session_event_model::InputId;

impl<P: InteractionPort> AgentSessionClient<P> {
    pub(crate) async fn prompt_content(
        &self,
        provider_session_id: String,
        input_id: InputId,
        operation_id: Option<P::OperationId>,
        prompt: crate::ProviderTurnInput,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        if let crate::ProviderTurnInput::Prompt(submission) = &prompt {
            let capabilities = self.capability_report(&provider_session_id).await;
            if let Err(error) = crate::ProviderPromptContent::new(
                crate::provider_prompt_content::acp_blocks_from_prompt_content(
                    submission.content.clone(),
                ),
                &capabilities,
            ) {
                if let Some(dispatch) = dispatch {
                    let observation = match &error {
                        ExternalProviderRuntimeError::UnsupportedContent { content_type } => {
                            ProviderPromptDispatchObservation::Refused(
                                crate::ProviderDispatchRefusal::UnsupportedContent(content_type),
                            )
                        }
                        _ => ProviderPromptDispatchObservation::NotSubmitted,
                    };
                    let _result = dispatch.send(observation);
                }
                return Err(error);
            }
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::Prompt {
                provider_session_id,
                input_id,
                operation_id,
                prompt,
                dispatch,
                reply,
            })
            .await
            .map_err(|error| {
                if let ProviderCommand::Prompt {
                    dispatch: Some(dispatch),
                    ..
                } = error.0
                {
                    let _result = dispatch.send(ProviderPromptDispatchObservation::NotSubmitted);
                }
                self.prompt_transport_failure()
            })?;
        result.await.map_err(|_| self.prompt_transport_failure())?
    }

    fn prompt_transport_failure(&self) -> ExternalProviderRuntimeError {
        if self.sink_closed.is_cancelled() {
            ExternalProviderRuntimeError::SinkClosed
        } else if self.frame_observation.limit_was_exceeded() {
            ExternalProviderRuntimeError::FrameLimitExceeded
        } else {
            ExternalProviderRuntimeError::TransportFailure
        }
    }
}
