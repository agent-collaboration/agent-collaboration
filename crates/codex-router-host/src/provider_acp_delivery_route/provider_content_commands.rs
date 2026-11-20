//! Host-internal typed prompt, steer, and queue admission.

use super::{
    LoadPolicy, OperationId, ProviderAcpDeliveryRoute, ProviderConversationBackend,
    ProviderIdentity, ProviderPromptContentsRequest, ProviderSessionLoadOutcome, RouteClaim,
    SessionRef, ensure_provider_session_loaded, session_queue_plan,
};
use crate::{ProviderSteeringOutcome, session_owner_calls::InputAcceptance};
use acp_client_runtime::RestorePhase;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderQueueAdmissionError {
    #[error("provider session not found")]
    SessionNotFound,
    #[error("provider session is unavailable")]
    Unavailable,
    #[error("provider session is live elsewhere")]
    LiveElsewhere,
    #[error("session settings are unresolved")]
    SettingsUnresolved,
    #[error("provider prompt content is empty")]
    EmptyContent,
    #[error("provider does not accept {content_type} content")]
    UnsupportedContent { content_type: &'static str },
    #[error("provider queue is full")]
    Busy,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderSteerContentsError {
    #[error(transparent)]
    Admission(#[from] ProviderQueueAdmissionError),
    #[error(transparent)]
    Runtime(#[from] crate::ExternalProviderRuntimeError),
    #[error("provider steer operation failed")]
    Operation(Box<collaboration_protocol::ConversationOperationFailure>),
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderPromptContentsError {
    #[error(transparent)]
    Admission(#[from] ProviderQueueAdmissionError),
    #[error("{reason}")]
    RestoreFailed { reason: String },
    #[error("provider prompt operation failed")]
    Operation(Box<collaboration_protocol::ConversationOperationFailure>),
}

impl ProviderAcpDeliveryRoute {
    pub async fn prompt_contents(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        input_id: session_event_model::InputId,
        contents: session_event_model::ProviderSubmission,
    ) -> Result<collaboration_protocol::ConversationOperationSubmission, ProviderPromptContentsError>
    {
        self.submit_input(
            target,
            actor,
            input_id,
            acp_client_runtime::ProviderTurnInput::Prompt(contents),
        )
        .await
    }

    pub async fn compact(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        input_id: session_event_model::InputId,
    ) -> Result<collaboration_protocol::ConversationOperationSubmission, ProviderPromptContentsError>
    {
        self.submit_input(
            target,
            actor,
            input_id,
            acp_client_runtime::ProviderTurnInput::Compact,
        )
        .await
    }

    async fn submit_input(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        input_id: session_event_model::InputId,
        input: acp_client_runtime::ProviderTurnInput,
    ) -> Result<collaboration_protocol::ConversationOperationSubmission, ProviderPromptContentsError>
    {
        if matches!(&input, acp_client_runtime::ProviderTurnInput::Prompt(submission) if submission.content.is_empty())
        {
            return Err(ProviderQueueAdmissionError::EmptyContent.into());
        }
        if !self.serves(&target) {
            return Err(ProviderQueueAdmissionError::SessionNotFound.into());
        }
        match self.claim.claim(&target).await.into_claim() {
            RouteClaim::Holds | RouteClaim::CanLoad => {}
            RouteClaim::Rejected { .. } | RouteClaim::LiveElsewhere { .. } => {
                return Err(ProviderQueueAdmissionError::LiveElsewhere.into());
            }
            RouteClaim::NotMine => return Err(ProviderQueueAdmissionError::SessionNotFound.into()),
            RouteClaim::Unavailable { .. } => {
                return Err(ProviderQueueAdmissionError::Unavailable.into());
            }
        }
        let record = self
            .store
            .lock()
            .await
            .session_record(&target)
            .await
            .map_err(|_| ProviderQueueAdmissionError::Unavailable)?
            .ok_or(ProviderQueueAdmissionError::SessionNotFound)?;
        match ensure_provider_session_loaded(
            &self.supervisor,
            &self.store,
            self.ownership.as_ref(),
            &target,
            LoadPolicy::MayLoad,
        )
        .await
        {
            ProviderSessionLoadOutcome::Ready | ProviderSessionLoadOutcome::AlreadyLoaded => {}
            ProviderSessionLoadOutcome::NotLoaded => {
                return Err(ProviderQueueAdmissionError::Unavailable.into());
            }
            ProviderSessionLoadOutcome::MissingRecord => {
                return Err(ProviderQueueAdmissionError::SessionNotFound.into());
            }
            ProviderSessionLoadOutcome::LiveElsewhere => {
                return Err(ProviderQueueAdmissionError::LiveElsewhere.into());
            }
            ProviderSessionLoadOutcome::Unavailable {
                phase: RestorePhase::Attempted,
                reason,
            } => {
                return Err(ProviderPromptContentsError::RestoreFailed { reason });
            }
            ProviderSessionLoadOutcome::Rejected {
                phase: RestorePhase::Attempted,
                reason,
            } => {
                return Err(ProviderPromptContentsError::RestoreFailed {
                    reason: reason.safe_detail(),
                });
            }
            ProviderSessionLoadOutcome::ClosedWhileLoading {
                phase: RestorePhase::Attempted,
            } => {
                return Err(ProviderPromptContentsError::RestoreFailed {
                    reason: crate::provider_acp_session_loading::closed_restore_reason(
                        RestorePhase::Attempted,
                    )
                    .to_owned(),
                });
            }
            _ => return Err(ProviderQueueAdmissionError::Unavailable.into()),
        }
        let runtime = self
            .supervisor
            .runtime_for(&target.endpoint)
            .ok_or(ProviderQueueAdmissionError::Unavailable)?;
        if self
            .supervisor
            .owner_session_snapshot(&target)
            .is_some_and(|snapshot| snapshot.settings_block_input)
        {
            return Err(ProviderQueueAdmissionError::SettingsUnresolved.into());
        }
        let capabilities = runtime
            .capability_report(&String::from(target.session_id.clone()))
            .await;
        if let acp_client_runtime::ProviderTurnInput::Prompt(submission) = &input
            && let Some(content_type) = unsupported_content_type(&submission.content, &capabilities)
        {
            return Err(ProviderQueueAdmissionError::UnsupportedContent { content_type }.into());
        }
        self.supervisor
            .prompt_contents(ProviderPromptContentsRequest {
                operation_id: OperationId::generate(),
                input_id,
                target,
                requested_by: actor,
                approver: record.approver,
                input,
                busy: crate::external_provider_supervisor::PromptBusyPolicy::RefuseBusy,
            })
            .await
            .map_err(|failure| ProviderPromptContentsError::Operation(Box::new(failure)))
    }

    pub async fn steer_contents(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        input_id: session_event_model::InputId,
        contents: session_event_model::ProviderSubmission,
    ) -> Result<ProviderSteeringOutcome, ProviderSteerContentsError> {
        Ok(
            match self
                .place_steer(target, actor, input_id, None, contents)
                .await?
            {
                InputAcceptance::Started | InputAcceptance::ProviderStarted => {
                    ProviderSteeringOutcome::StartedNewTurn
                }
                InputAcceptance::Steered { joined_operation } => {
                    ProviderSteeringOutcome::Injected {
                        running_operation_id: joined_operation
                            .and_then(|id| OperationId::try_from(id.to_string()).ok()),
                        joined_turn: None,
                    }
                }
                InputAcceptance::Unsent { .. } => ProviderSteeringOutcome::Failed,
                _ => return Err(ProviderQueueAdmissionError::Unavailable.into()),
            },
        )
    }

    pub(crate) async fn place_steer(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        input_id: session_event_model::InputId,
        expected_turn: Option<provider_session_machines::session_turn_machine::TurnId>,
        contents: session_event_model::ProviderSubmission,
    ) -> Result<InputAcceptance, ProviderSteerContentsError> {
        if contents.content.is_empty() {
            return Err(ProviderQueueAdmissionError::EmptyContent.into());
        }
        if !self.serves(&target) {
            return Err(ProviderQueueAdmissionError::SessionNotFound.into());
        }
        match self.claim.claim(&target).await.into_claim() {
            RouteClaim::Holds | RouteClaim::CanLoad => {}
            RouteClaim::Rejected { .. } | RouteClaim::LiveElsewhere { .. } => {
                return Err(ProviderQueueAdmissionError::LiveElsewhere.into());
            }
            RouteClaim::NotMine => return Err(ProviderQueueAdmissionError::SessionNotFound.into()),
            RouteClaim::Unavailable { .. } => {
                return Err(ProviderQueueAdmissionError::Unavailable.into());
            }
        }
        let record = self
            .store
            .lock()
            .await
            .session_record(&target)
            .await
            .map_err(|_| ProviderQueueAdmissionError::Unavailable)?
            .ok_or(ProviderQueueAdmissionError::SessionNotFound)?;
        let runtime = self
            .supervisor
            .runtime_for(&target.endpoint)
            .ok_or(ProviderQueueAdmissionError::Unavailable)?;
        if self
            .supervisor
            .owner_session_snapshot(&target)
            .is_some_and(|snapshot| snapshot.settings_block_input)
        {
            return Err(ProviderQueueAdmissionError::SettingsUnresolved.into());
        }
        let capabilities = runtime
            .capability_report(&String::from(target.session_id.clone()))
            .await;
        if let Some(content_type) = unsupported_content_type(&contents.content, &capabilities) {
            return Err(ProviderQueueAdmissionError::UnsupportedContent { content_type }.into());
        }
        let mode = collaboration_protocol::MessageDelivery::Steer;
        let request = ProviderPromptContentsRequest {
            operation_id: OperationId::generate(),
            input_id,
            target,
            requested_by: actor,
            approver: record.approver,
            input: acp_client_runtime::ProviderTurnInput::Prompt(contents),
            busy: crate::external_provider_supervisor::PromptBusyPolicy::Send(mode),
        };
        let result = if let Some(turn) = expected_turn {
            self.supervisor
                .submit_existing_turn_steer(request, turn)
                .await
        } else {
            self.supervisor.submit_owned_delivery(request, mode).await
        };
        result.map_err(|failure| ProviderSteerContentsError::Operation(Box::new(failure)))
    }
    pub async fn queue_contents(
        &self,
        target: SessionRef,
        actor: ProviderIdentity,
        contents: session_event_model::ProviderSubmission,
    ) -> Result<crate::ProviderQueuedInput, ProviderQueueAdmissionError> {
        if contents.content.is_empty() {
            return Err(ProviderQueueAdmissionError::EmptyContent);
        }
        if !self.serves(&target) {
            return Err(ProviderQueueAdmissionError::SessionNotFound);
        }
        match self.claim.claim(&target).await.into_claim() {
            RouteClaim::Holds | RouteClaim::CanLoad => {}
            RouteClaim::Rejected { .. } | RouteClaim::LiveElsewhere { .. } => {
                return Err(ProviderQueueAdmissionError::LiveElsewhere);
            }
            RouteClaim::NotMine => return Err(ProviderQueueAdmissionError::SessionNotFound),
            RouteClaim::Unavailable { .. } => return Err(ProviderQueueAdmissionError::Unavailable),
        }
        let record = self
            .store
            .lock()
            .await
            .session_record(&target)
            .await
            .map_err(|_| ProviderQueueAdmissionError::Unavailable)?
            .ok_or(ProviderQueueAdmissionError::SessionNotFound)?;
        let runtime = self
            .supervisor
            .runtime_for(&target.endpoint)
            .ok_or(ProviderQueueAdmissionError::Unavailable)?;
        if self
            .supervisor
            .owner_session_snapshot(&target)
            .is_some_and(|snapshot| snapshot.settings_block_input)
        {
            return Err(ProviderQueueAdmissionError::SettingsUnresolved);
        }
        let capabilities = runtime
            .capability_report(&String::from(target.session_id.clone()))
            .await;
        if let Some(content_type) = unsupported_content_type(&contents.content, &capabilities) {
            return Err(ProviderQueueAdmissionError::UnsupportedContent { content_type });
        }
        let binding = self
            .supervisor
            .binding(&target.endpoint)
            .ok_or(ProviderQueueAdmissionError::Unavailable)?;
        let operation_id = OperationId::generate();
        let run = match session_queue_plan(
            &self.supervisor,
            &self.store,
            self.ownership.as_ref(),
            &target,
            LoadPolicy::MayLoad,
        )
        .await
        {
            Ok(run) => run,
            Err(ProviderSessionLoadOutcome::MissingRecord) => {
                return Err(ProviderQueueAdmissionError::SessionNotFound);
            }
            Err(ProviderSessionLoadOutcome::LiveElsewhere) => {
                return Err(ProviderQueueAdmissionError::LiveElsewhere);
            }
            _ => return Err(ProviderQueueAdmissionError::Unavailable),
        };
        let input_id = session_event_model::InputId::generate();
        let queued = self
            .supervisor
            .queued_operation_registry()
            .record_queued_contents(
                operation_id.clone(),
                target.clone(),
                binding,
                input_id.clone(),
                &contents.content,
            );
        let request = ProviderPromptContentsRequest {
            operation_id,
            input_id,
            target: target.clone(),
            requested_by: actor,
            approver: record.approver,
            input: acp_client_runtime::ProviderTurnInput::Prompt(contents),
            busy: crate::external_provider_supervisor::PromptBusyPolicy::Send(
                collaboration_protocol::MessageDelivery::Queue,
            ),
        };
        let mut acceptance = self
            .supervisor
            .submit_queued_on_run(request.clone(), run)
            .await;
        if matches!(acceptance, Ok(InputAcceptance::QueuePlanChanged))
            && let Ok(run) = session_queue_plan(
                &self.supervisor,
                &self.store,
                self.ownership.as_ref(),
                &target,
                LoadPolicy::MayLoad,
            )
            .await
        {
            acceptance = self.supervisor.submit_queued_on_run(request, run).await;
        }
        let acceptance = acceptance.map_err(|failure| match failure.kind {
            collaboration_protocol::ConversationOperationFailureKind::Overloaded => {
                ProviderQueueAdmissionError::Busy
            }
            collaboration_protocol::ConversationOperationFailureKind::SettingsUnresolved => {
                ProviderQueueAdmissionError::SettingsUnresolved
            }
            _ => ProviderQueueAdmissionError::Unavailable,
        })?;
        if acceptance != crate::session_owner_calls::InputAcceptance::Queued {
            return Err(ProviderQueueAdmissionError::Unavailable);
        }
        Ok(queued)
    }
}

fn unsupported_content_type(
    contents: &[session_event_model::PromptContent],
    capabilities: &acp_client_runtime::ProviderCapabilityReport,
) -> Option<&'static str> {
    contents.iter().find_map(|content| match content {
        session_event_model::PromptContent::Text { .. }
        | session_event_model::PromptContent::ResourceLink { .. } => None,
        session_event_model::PromptContent::Image { .. } if !capabilities.accepts_image => {
            Some("image")
        }
        session_event_model::PromptContent::Audio { .. } if !capabilities.accepts_audio => {
            Some("audio")
        }
        session_event_model::PromptContent::EmbeddedResource { .. }
            if !capabilities.accepts_embedded_resource =>
        {
            Some("embeddedResource")
        }
        _ => None,
    })
}
