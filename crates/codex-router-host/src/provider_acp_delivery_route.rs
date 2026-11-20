//! ACP provider message delivery, evidence, and reconciliation.
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_contracts::NativeRefusalCode;
use collaboration_contracts::UnsentCause;
mod native_result;
mod provider_active_turn_cancel;
mod provider_content_commands;
mod provider_delivery_request;
mod session_delivery_route;
pub use provider_active_turn_cancel::ProviderCancelActiveTurnError;
pub use provider_content_commands::{
    ProviderPromptContentsError, ProviderQueueAdmissionError, ProviderSteerContentsError,
};
use provider_delivery_request::{ProviderDeliveryContent, ProviderDeliveryRequest};

use crate::external_provider_supervisor::ProviderPromptContentsRequest;
use crate::provider_acp_route_claim::ProviderAcpRouteClaim;
use crate::provider_acp_session_loading::{
    ProviderSessionLoadOutcome, ProviderSessionLoadRejection, ensure_provider_session_loaded,
    session_queue_plan,
};
use crate::{ExternalProviderSupervisor, LiveSessionOwnershipCheck};
use agent_automation::{
    ProviderAcpEffectEvidence, ProviderBindingReference, ProviderSettlementEffect,
    RouteEffectEvidence, SubmissionEffect,
};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::NOT_LOADED_REASON;
use collaboration_contracts::ProviderConversationBackend;
use collaboration_contracts::RouteClaim;
use collaboration_protocol::{
    CodexGeneration, DeliveryClientReceipt, DeliveryNextAction, DeliveryRejection,
    DeliveryRejectionReason, MessageDelivery, OperationId, ProviderIdentity,
    ProviderOperationEffect, SessionReachability, SessionRef, UuidIdentity,
};
use collaboration_runtime::EndpointDirectory;
use collaboration_runtime::ProviderOperationStore;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;

pub struct ProviderAcpDeliveryRoute {
    service_id: UuidIdentity,
    claim: ProviderAcpRouteClaim,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    #[cfg(feature = "test-support")]
    queue_plan_pause: std::sync::Mutex<Option<QueuePlanPause>>,
}

#[cfg(feature = "test-support")]
struct QueuePlanPause {
    sampled: tokio::sync::oneshot::Sender<acp_client_runtime::acp_transport::TransportRun>,
    release: tokio::sync::oneshot::Receiver<()>,
}

impl ProviderAcpDeliveryRoute {
    #[must_use]
    pub fn queue_list(&self, session: &SessionRef) -> Vec<crate::ProviderQueuedInput> {
        let Some(snapshot) = self.supervisor.owner_session_snapshot(session) else {
            return Vec::new();
        };
        self.supervisor
            .queued_operation_registry()
            .list_in_owner_order(
                session,
                snapshot
                    .queue
                    .into_iter()
                    .filter(|entry| !entry.started)
                    .map(|entry| entry.input.input),
            )
    }

    pub async fn queue_cancel(
        &self,
        session: &SessionRef,
        input_id: &session_event_model::InputId,
    ) -> Result<(), crate::ProviderQueueCancellationError> {
        let owner = self
            .supervisor
            .session_owner_for(session)
            .map_err(|_| crate::ProviderQueueCancellationError::NotQueued)?;
        owner
            .cancel_queued(input_id.clone())
            .await
            .map_err(|_| crate::ProviderQueueCancellationError::NotQueued)
    }
    #[must_use]
    pub fn new(
        service_id: UuidIdentity,
        provider_endpoints: HashSet<collaboration_protocol::EndpointRef>,
        directory: EndpointDirectory,
        supervisor: Arc<ExternalProviderSupervisor>,
        store: Arc<Mutex<ProviderOperationStore>>,
        ownership: Arc<dyn LiveSessionOwnershipCheck>,
        _clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        let claim = ProviderAcpRouteClaim::new(
            service_id.clone(),
            provider_endpoints,
            directory,
            Arc::clone(&supervisor),
            Arc::clone(&store),
        );
        Self {
            service_id,
            claim,
            supervisor,
            store,
            ownership,
            #[cfg(feature = "test-support")]
            queue_plan_pause: std::sync::Mutex::new(None),
        }
    }

    #[cfg(feature = "test-support")]
    pub fn pause_next_queue_plan_for_fixture(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<acp_client_runtime::acp_transport::TransportRun>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (sampled, observed) = tokio::sync::oneshot::channel();
        let (resume, release) = tokio::sync::oneshot::channel();
        let previous = self
            .queue_plan_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(QueuePlanPause { sampled, release });
        assert!(previous.is_none(), "a fixture pause is still pending");
        (observed, resume)
    }

    pub async fn shutdown_queue(&self) {
        self.supervisor.cancel_waiting_owner_inputs().await;
    }

    fn serves(&self, target: &SessionRef) -> bool {
        self.claim.serves(target)
    }

    fn effect(
        request: &ProviderDeliveryRequest,
        binding: &collaboration_protocol::ProviderBindingIdentity,
        submission: SubmissionEffect,
    ) -> Result<RouteEffectEvidence<SessionRef, CodexGeneration>, DeliveryContractError> {
        let reference =
            ProviderBindingReference::try_from(String::from(binding.binding_id.clone()))
                .map_err(|_| DeliveryContractError::InvalidEvidence)?;
        Ok(RouteEffectEvidence::ProviderAcp(
            ProviderAcpEffectEvidence {
                target: request.target.clone(),
                generation: binding.generation.clone(),
                binding: reference,
                attempt_id: request.attempt.clone(),
                submission,
                settlement: ProviderSettlementEffect::NotObserved,
            },
        ))
    }

    async fn deliver_provider<'a>(
        &'a self,
        request: ProviderDeliveryRequest,
        sink: &'a dyn AttemptEvidenceSink,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        let input_id = request.input_id()?;
        if !self.serves(&request.target) {
            return Ok(Self::rejected(
                DeliveryRejectionReason::NoRoute,
                "provider route does not serve this endpoint",
            ));
        }
        let Some(binding) = self.supervisor.binding(&request.target.endpoint) else {
            return Ok(Self::not_submitted(
                "provider runtime is unavailable",
                false,
                UnsentCause::Refused(NativeRefusalCode::RuntimeMissing),
            ));
        };
        if let DeliveryPrecondition::EndpointGeneration { expected } = &request.precondition
            && expected != &binding.generation
        {
            return Ok(Self::rejected(
                DeliveryRejectionReason::StaleGeneration,
                "provider binding generation changed",
            ));
        }
        let claim = self.claim.claim(&request.target).await;
        let claim_cause = claim.unsent_cause();
        match claim.into_claim() {
            RouteClaim::Holds => {}
            RouteClaim::CanLoad => {}
            RouteClaim::Unavailable { reason, retryable } => {
                return Ok(Self::not_submitted(
                    reason.reason,
                    retryable,
                    claim_cause.ok_or(DeliveryContractError::InvalidEvidence)?,
                ));
            }
            RouteClaim::Rejected { .. } | RouteClaim::LiveElsewhere { .. } => {
                return Ok(Self::not_submitted(
                    "session is live elsewhere",
                    true,
                    claim_cause.ok_or(DeliveryContractError::InvalidEvidence)?,
                ));
            }
            RouteClaim::NotMine => {
                return Ok(Self::rejected(
                    DeliveryRejectionReason::NoRoute,
                    "provider has no recorded session",
                ));
            }
        }
        let operation_id = OperationId::try_from(request.attempt.as_str().to_owned())
            .map_err(|_| DeliveryContractError::InvalidEvidence)?;
        let mut effect = Self::effect(&request, &binding, SubmissionEffect::Dispatching)?;
        let (loaded, queue_run) = if request.mode == MessageDelivery::Queue {
            match session_queue_plan(
                &self.supervisor,
                &self.store,
                self.ownership.as_ref(),
                &request.target,
                request.load_policy,
            )
            .await
            {
                Ok(run) => (ProviderSessionLoadOutcome::Ready, Some(run)),
                Err(reason) => (reason, None),
            }
        } else {
            (
                ensure_provider_session_loaded(
                    &self.supervisor,
                    &self.store,
                    self.ownership.as_ref(),
                    &request.target,
                    request.load_policy,
                )
                .await,
                None,
            )
        };
        #[cfg(feature = "test-support")]
        if let Some(plan) = queue_run {
            let pause = self
                .queue_plan_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                let _fixture_gone = pause.sampled.send(plan.run());
                let _released = pause.release.await;
            }
        }
        match loaded {
            ProviderSessionLoadOutcome::Ready | ProviderSessionLoadOutcome::AlreadyLoaded => {}
            ProviderSessionLoadOutcome::NotLoaded => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        NOT_LOADED_REASON,
                        true,
                        UnsentCause::Holdable {
                            otherwise: NativeRefusalCode::ThreadNotLoaded,
                        },
                    )
                    .await;
            }
            // A close won while the load was in flight: this delivery ends
            // not sent, never held.
            ProviderSessionLoadOutcome::ClosedWhileLoading { phase } => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        crate::provider_acp_session_loading::closed_restore_reason(phase),
                        false,
                        UnsentCause::Refused(NativeRefusalCode::Rejection(
                            DeliveryRejectionReason::NotResumable,
                        )),
                    )
                    .await;
            }
            ProviderSessionLoadOutcome::UnsupportedLoad => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        "unsupported: load",
                        false,
                        UnsentCause::Refused(NativeRefusalCode::Rejection(
                            DeliveryRejectionReason::UnsupportedCapability,
                        )),
                    )
                    .await;
            }
            ProviderSessionLoadOutcome::MissingRecord => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        "provider session record is missing",
                        false,
                        UnsentCause::RecipientUnknown,
                    )
                    .await;
            }
            ProviderSessionLoadOutcome::LiveElsewhere => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        "session became live elsewhere",
                        true,
                        UnsentCause::Holdable {
                            otherwise: NativeRefusalCode::Rejection(
                                DeliveryRejectionReason::LiveElsewhere,
                            ),
                        },
                    )
                    .await;
            }
            ProviderSessionLoadOutcome::Unavailable { phase, reason } => {
                use acp_client_runtime::RestorePhase;
                let (retryable, cause) = match phase {
                    RestorePhase::Preflight => (
                        true,
                        UnsentCause::Holdable {
                            otherwise: NativeRefusalCode::NativeUnavailable,
                        },
                    ),
                    RestorePhase::Attempted => (
                        false,
                        UnsentCause::Refused(NativeRefusalCode::NativeUnavailable),
                    ),
                };
                return self
                    .finish_known_none(&request, sink, &mut effect, reason, retryable, cause)
                    .await;
            }
            ProviderSessionLoadOutcome::Rejected { reason, .. } => {
                return self.finish_rejected(sink, &mut effect, reason).await;
            }
        }
        let record = match self
            .store
            .lock()
            .await
            .session_record(&request.target)
            .await
        {
            Ok(Some(record)) => record,
            Ok(None) => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        "provider session record is missing",
                        false,
                        UnsentCause::RecipientUnknown,
                    )
                    .await;
            }
            Err(_) => return self.finish_unknown(sink, &mut effect).await,
        };
        let requested_by = match &request.content {
            ProviderDeliveryContent::PreparedPush { .. } => record.created_by.clone(),
        };
        let contents_request = match &request.content {
            ProviderDeliveryContent::PreparedPush { line, .. } => {
                ProviderPromptContentsRequest::from_prepared_push(
                    operation_id.clone(),
                    input_id,
                    request.target.clone(),
                    requested_by,
                    record.approver,
                    line,
                )?
            }
        };
        use crate::session_owner_calls::InputAcceptance;
        let mut dispatch = self
            .supervisor
            .submit_evidenced_delivery(
                contents_request.clone(),
                request.mode,
                queue_run,
                sink,
                effect.clone(),
            )
            .await;
        if matches!(dispatch, Ok(InputAcceptance::QueuePlanChanged))
            && let Ok(run) = session_queue_plan(
                &self.supervisor,
                &self.store,
                self.ownership.as_ref(),
                &request.target,
                request.load_policy,
            )
            .await
        {
            dispatch = self
                .supervisor
                .submit_evidenced_delivery(
                    contents_request,
                    request.mode,
                    Some(run),
                    sink,
                    effect.clone(),
                )
                .await;
        }
        let (observed, operation, submission) = match dispatch {
            Ok(InputAcceptance::Started) => (
                NativeObservedOutcome::Started,
                Some(operation_id),
                SubmissionEffect::Accepted,
            ),
            Ok(InputAcceptance::ProviderStarted) => (
                NativeObservedOutcome::Started,
                None,
                SubmissionEffect::Accepted,
            ),
            Ok(InputAcceptance::Steered { joined_operation }) => (
                NativeObservedOutcome::Steered,
                joined_operation.and_then(|id| OperationId::try_from(id.to_string()).ok()),
                SubmissionEffect::Accepted,
            ),
            Ok(InputAcceptance::Queued) => {
                return Ok(Self::observed(NativeObservedOutcome::Queued, Some(operation_id)));
            }
            Ok(InputAcceptance::QueuePlanChanged) => {
                return self.finish_known_none(&request, sink, &mut effect,
                    "the session changed state while the message was being queued; send again",
                    false, UnsentCause::Refused(NativeRefusalCode::ProviderNotSubmitted),
                ).await;
            }
            Ok(InputAcceptance::Unsent {
                reason: Some(reason @ provider_session_machines::session_owner::RevokeReason::DeliveryEvidenceUnavailable),
            }) => {
                return Ok(Self::not_submitted(
                    reason.human_phrase(),
                    false,
                    UnsentCause::Refused(NativeRefusalCode::ProviderNotSubmitted),
                ));
            }
            Ok(InputAcceptance::Unsent { reason }) => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        reason.map_or_else(|| "provider prompt was not sent".into(),
                            provider_session_machines::session_owner::RevokeReason::display_detail),
                        false,
                        UnsentCause::Refused(NativeRefusalCode::ProviderNotSubmitted),
                    )
                    .await;
            }
            Err(failure) if failure.kind == collaboration_protocol::ConversationOperationFailureKind::CannotSteer => {
                return Ok(Self::rejected(DeliveryRejectionReason::SteerUnsupported, &String::from(failure.message)));
            }
            Err(failure) if failure.effect == ProviderOperationEffect::None => {
                return self
                    .finish_known_none(
                        &request,
                        sink,
                        &mut effect,
                        String::from(failure.message),
                        true,
                        UnsentCause::Holdable {
                            otherwise: NativeRefusalCode::ProviderNotSubmitted,
                        },
                    )
                    .await;
            }
            Ok(InputAcceptance::Unknown) | Err(_) => {
                return self.finish_unknown(sink, &mut effect).await;
            }
        };
        update_submission(&mut effect, submission);
        if sink.record(effect).await.is_err() {
            return Ok(Self::observed(NativeObservedOutcome::Unknown, None));
        }
        Ok(Self::observed(observed, operation))
    }
}

fn update_submission(
    effect: &mut RouteEffectEvidence<SessionRef, CodexGeneration>,
    submission: SubmissionEffect,
) {
    if let RouteEffectEvidence::ProviderAcp(provider) = effect {
        provider.submission = submission;
    }
}
