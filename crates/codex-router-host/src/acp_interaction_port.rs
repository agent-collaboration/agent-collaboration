//! Host policy adapter for the ACP client's provider interactions.

use provider_session_machines::SessionGeneration;
use std::sync::{Arc, Weak};
use std::time::Duration;

use acp_client_runtime::{
    ApprovalPortOutcome, InteractionFuture, InteractionPort, RefusedApprovalOffer, SessionEventSink,
};
use collaboration_protocol::OperationId;
use collaboration_runtime::RefusedApprovalOption;
use collaboration_runtime::RefusedTypedApproval;
use collaboration_runtime::ServiceInteractionBroker;
use message_board::{Identity, SessionRef as BoardSessionRef};
use session_event_model::{
    ApprovalRequest, PendingInteraction, QuestionRequest, QuestionResponse, SessionEvent,
};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::ExternalProviderApprovalContext;

const APPROVAL_DECISION_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Default)]
pub(crate) struct HostInteractionPort {
    broker: Arc<RwLock<Option<Weak<ServiceInteractionBroker>>>>,
    event_sink: Arc<RwLock<Option<Arc<dyn SessionEventSink>>>>,
}

impl HostInteractionPort {
    pub(crate) async fn install_broker(&self, broker: Arc<ServiceInteractionBroker>) {
        *self.broker.write().await = Some(Arc::downgrade(&broker));
    }

    async fn broker(&self) -> Option<Arc<ServiceInteractionBroker>> {
        self.broker.read().await.as_ref().and_then(Weak::upgrade)
    }

    pub(crate) async fn install_event_sink(&self, event_sink: Arc<dyn SessionEventSink>) {
        *self.event_sink.write().await = Some(event_sink);
    }

    async fn publication_scope(
        &self,
        session_id: &str,
        context: &ExternalProviderApprovalContext,
    ) -> PublicationScope {
        if let Some(scope) = context.owner_scope {
            return PublicationScope::Turn(scope);
        }
        self.event_sink
            .read()
            .await
            .as_ref()
            .map_or(PublicationScope::Current, |sink| {
                PublicationScope::Run(sink.current_generation(session_id))
            })
    }

    async fn publish(
        &self,
        session_id: &str,
        scope: PublicationScope,
        event: SessionEvent,
    ) -> bool {
        use acp_client_runtime::GenerationPublication;
        let Some(sink) = self.event_sink.read().await.clone() else {
            return true;
        };
        let published = match scope {
            PublicationScope::Turn(expected) => sink
                .publish_for_turn(session_id, expected.run, expected.turn, event)
                .map(|result| result == GenerationPublication::Published),
            PublicationScope::Run(generation) => sink
                .publish_in(session_id, generation, event)
                .map(|result| result == GenerationPublication::Published),
            PublicationScope::Current => sink.publish(session_id, event).map(|()| true),
        };
        if published.is_err() {
            tracing::error!(
                session_id,
                "provider interaction event could not be published"
            );
        }
        published.unwrap_or(false)
    }
}

#[derive(Clone, Copy)]
enum PublicationScope {
    Current,
    Run(SessionGeneration),
    Turn(crate::external_provider_approval_context::OwnerApprovalScope),
}

impl InteractionPort for HostInteractionPort {
    type Context = ExternalProviderApprovalContext;
    type OperationId = OperationId;

    fn bind_owner_scope(
        context: &mut Self::Context,
        run: SessionGeneration,
        turn: provider_session_machines::session_turn_machine::TurnId,
    ) {
        context.owner_scope =
            Some(crate::external_provider_approval_context::OwnerApprovalScope { run, turn });
    }
    fn operation_id(context: &Self::Context) -> Self::OperationId {
        context.operation_id.clone()
    }

    fn binding_retirement(context: &Self::Context) -> CancellationToken {
        context.binding_retirement.clone()
    }

    fn request_approval(
        &self,
        context: Self::Context,
        request: ApprovalRequest,
        turn_cancellation: CancellationToken,
        agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async move {
            let Some(broker) = self.broker().await else {
                return ApprovalPortOutcome::Unavailable;
            };
            let Some((requester, approver)) = typed_participants(&context) else {
                return ApprovalPortOutcome::Unavailable;
            };
            let request_id = request.request_id.clone();
            let session_id = String::from(context.target.session_id.clone());
            let scope = self.publication_scope(&session_id, &context).await;
            let pending_event = SessionEvent::InteractionRequested {
                interaction: PendingInteraction::Approval {
                    approver: approver.clone(),
                    request: Box::new(request.clone()),
                },
            };
            let receiver = match broker
                .request_typed_approval(
                    requester.clone(),
                    approver.clone(),
                    request.clone(),
                    turn_cancellation.clone(),
                    context.binding_retirement.clone(),
                    Some(collaboration_runtime::TypedApprovalLegacyContext {
                        operation_id: context.operation_id.clone(),
                        target: context.target.clone(),
                        generation: context.binding_generation.clone(),
                        requested_by: context.requester.clone(),
                    }),
                )
                .await
            {
                Ok(receiver) => receiver,
                Err(collaboration_runtime::InteractionHistoryError::NotPending) => {
                    return ApprovalPortOutcome::Cancelled;
                }
                Err(collaboration_runtime::InteractionHistoryError::SelfApprover) => {
                    let refusal = RefusedTypedApproval {
                        request_id: request.request_id,
                        title: request.title,
                        description: request.description,
                        subject: request.subject,
                        options: request
                            .options
                            .iter()
                            .map(|option| RefusedApprovalOption {
                                option_id: option.option_id.as_str().to_owned(),
                                label: option.label.clone(),
                                provider_kind: "selfApprover".to_owned(),
                            })
                            .collect(),
                        reason: "set a different approver".to_owned(),
                    };
                    if let Err(error) = broker
                        .record_typed_refusal(requester, approver, refusal)
                        .await
                    {
                        tracing::error!(%error, "self-approver refusal could not be recorded");
                    }
                    return ApprovalPortOutcome::Unavailable;
                }
                Err(error) => {
                    tracing::error!(%error, "provider approval could not be admitted");
                    return ApprovalPortOutcome::Unavailable;
                }
            };
            if !self.publish(&session_id, scope, pending_event).await {
                let _ = broker
                    .cancel_typed_approval(&request_id, "eventPublicationFailed")
                    .await;
                return ApprovalPortOutcome::Unavailable;
            }
            tokio::pin!(receiver);
            let (outcome, resolved_here) = tokio::select! {
                biased;
                () = turn_cancellation.cancelled() => {
                    let settled = broker.cancel_typed_approval(&request_id, "turnCancelled").await.is_ok();
                    (approval_receiver_outcome(receiver.await), settled)
                }
                () = context.binding_retirement.cancelled() => {
                    let settled = broker.cancel_typed_approval(&request_id, "providerRetired").await.is_ok();
                    (approval_receiver_outcome(receiver.await), settled)
                }
                () = agent_cancellation.cancelled() => {
                    let settled = broker.cancel_typed_approval(&request_id, "agentCancelled").await.is_ok();
                    (approval_receiver_outcome(receiver.await), settled)
                }
                () = tokio::time::sleep(APPROVAL_DECISION_TIMEOUT) => {
                    let settled = broker.cancel_typed_approval(&request_id, "timedOut").await.is_ok();
                    (approval_receiver_outcome(receiver.await), settled)
                }
                selected = &mut receiver => {
                    let resolved_here = selected.is_ok()
                        || (!turn_cancellation.is_cancelled()
                            && !context.binding_retirement.is_cancelled()
                            && !agent_cancellation.is_cancelled());
                    (approval_receiver_outcome(selected), resolved_here)
                },
            };
            if resolved_here {
                let _ = self
                    .publish(
                        &session_id,
                        scope,
                        SessionEvent::InteractionResolved { request_id },
                    )
                    .await;
            }
            outcome
        })
    }

    fn request_question(
        &self,
        context: Self::Context,
        request: QuestionRequest,
        turn_cancellation: CancellationToken,
        agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        Box::pin(async move {
            let Some(broker) = self.broker().await else {
                return QuestionResponse::Cancelled;
            };
            let Some((requester, approver)) = typed_participants(&context) else {
                return QuestionResponse::Cancelled;
            };
            if turn_cancellation.is_cancelled()
                || agent_cancellation.is_cancelled()
                || context.binding_retirement.is_cancelled()
            {
                let reason = if turn_cancellation.is_cancelled() {
                    "turnCancelled"
                } else if context.binding_retirement.is_cancelled() {
                    "providerRetired"
                } else {
                    "agentCancelled"
                };
                if let Err(error) = broker
                    .record_cancelled_question(requester, approver, request, reason)
                    .await
                {
                    tracing::error!(%error, "cancelled provider question could not be recorded");
                }
                return QuestionResponse::Cancelled;
            }
            let request_id = request.request_id.clone();
            let session_id = String::from(context.target.session_id.clone());
            let scope = self.publication_scope(&session_id, &context).await;
            let pending_event = SessionEvent::InteractionRequested {
                interaction: PendingInteraction::Question {
                    approver: approver.clone(),
                    request: Box::new(request.clone()),
                },
            };
            let receiver = match broker
                .request_question(
                    requester,
                    approver,
                    request,
                    Some(context.binding_retirement.clone()),
                )
                .await
            {
                Ok(receiver) => receiver,
                Err(error) => {
                    tracing::error!(%error, "provider question could not be admitted");
                    return QuestionResponse::Cancelled;
                }
            };
            if !self.publish(&session_id, scope, pending_event).await {
                let _ = broker
                    .cancel_question(&request_id, "eventPublicationFailed")
                    .await;
                return QuestionResponse::Cancelled;
            }
            tokio::pin!(receiver);
            let (response, resolved_here) = tokio::select! {
                biased;
                () = turn_cancellation.cancelled() => {
                    let settled = broker.cancel_question(&request_id, "turnCancelled").await.is_ok();
                    (receiver.await.unwrap_or(QuestionResponse::Cancelled), settled)
                }
                () = context.binding_retirement.cancelled() => {
                    let settled = broker.cancel_question(&request_id, "providerRetired").await.is_ok();
                    (receiver.await.unwrap_or(QuestionResponse::Cancelled), settled)
                }
                () = agent_cancellation.cancelled() => {
                    let settled = broker.cancel_question(&request_id, "agentCancelled").await.is_ok();
                    (receiver.await.unwrap_or(QuestionResponse::Cancelled), settled)
                }
                settled = &mut receiver => {
                    let response = settled.unwrap_or(QuestionResponse::Cancelled);
                    let resolved_here = !turn_cancellation.is_cancelled()
                        && !context.binding_retirement.is_cancelled()
                        && !agent_cancellation.is_cancelled();
                    (response, resolved_here)
                },
            };
            if resolved_here {
                let _ = self
                    .publish(
                        &session_id,
                        scope,
                        SessionEvent::InteractionResolved { request_id },
                    )
                    .await;
            }
            response
        })
    }

    fn record_refusal(
        &self,
        context: Self::Context,
        refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async move {
            let Some(broker) = self.broker().await else {
                tracing::error!(
                    "provider approval refusal could not be recorded: broker unavailable"
                );
                return;
            };
            let Some((requester, approver)) = typed_participants(&context) else {
                tracing::error!(
                    "provider approval refusal could not be recorded: invalid Session identity"
                );
                return;
            };
            let record = RefusedTypedApproval {
                request_id: refusal.request_id,
                title: refusal.title,
                description: refusal.description,
                subject: refusal.subject,
                options: refusal
                    .options
                    .into_iter()
                    .map(|option| RefusedApprovalOption {
                        option_id: option.option_id,
                        label: option.label,
                        provider_kind: option.provider_kind,
                    })
                    .collect(),
                reason: refusal.reason.to_owned(),
            };
            if let Err(error) = broker
                .record_typed_refusal(requester, approver, record)
                .await
            {
                tracing::error!(%error, "provider approval refusal could not be recorded");
            }
        })
    }

    fn cancel_all(
        &self,
        context: Self::Context,
        reason: &'static str,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async move {
            let Some(broker) = self.broker().await else {
                return;
            };
            let Some((requester, _)) = typed_participants(&context) else {
                return;
            };
            match broker.cancel_typed_approvals(&requester, reason).await {
                Ok(request_ids) => {
                    let session_id = String::from(context.target.session_id.clone());
                    let scope = self.publication_scope(&session_id, &context).await;
                    for request_id in request_ids {
                        let _ = self
                            .publish(
                                &session_id,
                                scope,
                                SessionEvent::InteractionResolved { request_id },
                            )
                            .await;
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "failed to cancel provider approvals for Session")
                }
            }
            match broker.cancel_questions(&requester, reason).await {
                Ok(request_ids) => {
                    let session_id = String::from(context.target.session_id.clone());
                    let scope = self.publication_scope(&session_id, &context).await;
                    for request_id in request_ids {
                        let _ = self
                            .publish(
                                &session_id,
                                scope,
                                SessionEvent::InteractionResolved { request_id },
                            )
                            .await;
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "failed to cancel provider questions for Session")
                }
            }
        })
    }

    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async move {
            if let Some(broker) = self.broker().await
                && let Err(error) = broker.cancel_retired_typed_approvals().await
            {
                tracing::error!(%error, "failed to cancel retired provider approvals");
            }
        })
    }
}

fn typed_participants(
    context: &ExternalProviderApprovalContext,
) -> Option<(BoardSessionRef, Identity)> {
    let requester = board_session_ref(&context.target)?;
    let approver = context.approver.to_board_identity().ok()?;
    Some((requester, approver))
}

fn board_session_ref(value: &collaboration_protocol::SessionRef) -> Option<BoardSessionRef> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn approval_receiver_outcome(
    selected: Result<
        collaboration_runtime::TypedApprovalResolution,
        tokio::sync::oneshot::error::RecvError,
    >,
) -> ApprovalPortOutcome {
    match selected {
        Ok(collaboration_runtime::TypedApprovalResolution::Selected(selected)) => {
            ApprovalPortOutcome::Selected {
                option_id: selected.option_id.as_str().to_owned(),
                note: selected.note,
            }
        }
        Ok(collaboration_runtime::TypedApprovalResolution::Cancelled) => {
            ApprovalPortOutcome::Cancelled
        }
        Err(_) => ApprovalPortOutcome::Cancelled,
    }
}

#[cfg(test)]
#[path = "acp_interaction_port_test.rs"]
mod provider_actor_tests;

#[cfg(test)]
#[path = "acp_interaction_port_node_tap_test.rs"]
mod node_tap_tests;
