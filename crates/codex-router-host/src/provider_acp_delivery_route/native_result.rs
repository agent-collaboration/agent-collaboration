//! Native outcome and evidence construction has one owning boundary.
use super::{
    AttemptEvidenceSink, CodexGeneration, DeliveryClientReceipt, DeliveryContractError,
    DeliveryNextAction, DeliveryRejection, DeliveryRejectionReason, NativeDeliveryResult,
    NativeObservedOutcome, OperationId, ProviderAcpDeliveryRoute, ProviderDeliveryRequest,
    ProviderSessionLoadRejection, RouteEffectEvidence, SessionReachability, SessionRef,
    SubmissionEffect, UnsentCause, update_submission,
};
impl ProviderAcpDeliveryRoute {
    pub(super) fn observed(
        outcome: NativeObservedOutcome,
        operation_id: Option<OperationId>,
    ) -> NativeDeliveryResult {
        NativeDeliveryResult::observed(
            outcome,
            Some(SessionReachability::ProviderAcp),
            operation_id.map(|operation_id| DeliveryClientReceipt::ProviderAcp { operation_id }),
        )
    }

    pub(super) fn rejected(reason: DeliveryRejectionReason, detail: &str) -> NativeDeliveryResult {
        let next_action = match reason {
            DeliveryRejectionReason::Busy | DeliveryRejectionReason::EndpointUnavailable => {
                DeliveryNextAction::RetryLater
            }
            DeliveryRejectionReason::LiveElsewhere => DeliveryNextAction::InspectTarget,
            _ => DeliveryNextAction::CorrectRequest,
        };
        NativeDeliveryResult::rejected(
            DeliveryRejection {
                reason,
                next_action,
                client_code: None,
                detail: Some(detail.to_owned()),
                claims: None,
                native_failure: None,
            },
            UnsentCause::for_rejection(reason),
        )
        .with_reachability(SessionReachability::ProviderAcp)
    }

    pub(super) fn not_submitted(
        reason: impl Into<String>,
        retryable: bool,
        cause: UnsentCause,
    ) -> NativeDeliveryResult {
        NativeDeliveryResult::not_submitted(reason, retryable, cause)
            .with_reachability(SessionReachability::ProviderAcp)
    }

    pub(super) async fn finish_known_none(
        &self,
        _request: &ProviderDeliveryRequest,
        sink: &dyn AttemptEvidenceSink,
        effect: &mut RouteEffectEvidence<SessionRef, CodexGeneration>,
        reason: impl Into<String>,
        retryable: bool,
        cause: UnsentCause,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        update_submission(effect, SubmissionEffect::NotDispatched);
        if sink.record(effect.clone()).await.is_err() {
            return Ok(Self::observed(NativeObservedOutcome::Unknown, None));
        }
        Ok(Self::not_submitted(reason, retryable, cause))
    }

    pub(super) async fn finish_rejected(
        &self,
        sink: &dyn AttemptEvidenceSink,
        effect: &mut RouteEffectEvidence<SessionRef, CodexGeneration>,
        provider_reason: ProviderSessionLoadRejection,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        update_submission(effect, SubmissionEffect::NotDispatched);
        if sink.record(effect.clone()).await.is_err() {
            return Ok(Self::observed(NativeObservedOutcome::Unknown, None));
        }
        let client_code = Some(provider_reason.code());
        let detail = Some(provider_reason.safe_detail());
        let (reason, next_action) = match provider_reason {
            ProviderSessionLoadRejection::SessionNotFound { .. } => (
                DeliveryRejectionReason::ProviderSessionNotFound,
                DeliveryNextAction::CorrectRequest,
            ),
            ProviderSessionLoadRejection::ProviderRejected { .. } => (
                DeliveryRejectionReason::ProviderRejected,
                DeliveryNextAction::InspectTarget,
            ),
        };
        Ok(NativeDeliveryResult::rejected(
            DeliveryRejection {
                reason,
                next_action,
                client_code,
                detail,
                claims: None,
                native_failure: None,
            },
            UnsentCause::for_rejection(reason),
        )
        .with_reachability(SessionReachability::ProviderAcp))
    }

    pub(super) async fn finish_unknown(
        &self,
        sink: &dyn AttemptEvidenceSink,
        effect: &mut RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        update_submission(effect, SubmissionEffect::Unknown);
        let _result = sink.record(effect.clone()).await;
        Ok(Self::observed(NativeObservedOutcome::Unknown, None))
    }
}
