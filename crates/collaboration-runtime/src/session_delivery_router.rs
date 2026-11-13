//! Selects one injected client route for an attempt, then keeps that choice fixed.
use agent_automation::RouteEffectEvidence;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::RouteClaim;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_contracts::UnsentCause;
use collaboration_protocol::{
    DeliveryNextAction, DeliveryRejection, DeliveryRejectionReason, MessageDelivery,
    SessionReachability, SessionRef,
};
use futures_util::future::join_all;
use std::sync::Arc;

enum RouteDecision {
    Selected(usize),
    Complete(Box<NativeDeliveryResult>),
}

pub struct SessionDeliveryRouter {
    pub(crate) routes: Vec<Arc<dyn SessionDeliveryRoute>>,
}

impl SessionDeliveryRouter {
    #[must_use]
    pub fn new(routes: Vec<Arc<dyn SessionDeliveryRoute>>) -> Self {
        Self { routes }
    }

    async fn deliver_once(
        &self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        evidence: &dyn AttemptEvidenceSink,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        match self.select_route(&request.target).await? {
            RouteDecision::Selected(index) => self.deliver_through(index, request, evidence).await,
            RouteDecision::Complete(receipt) => Ok(*receipt),
        }
    }

    async fn select_route(
        &self,
        target: &SessionRef,
    ) -> Result<RouteDecision, DeliveryContractError> {
        let claims = join_all(self.routes.iter().map(|route| route.claim(target)))
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        if let Some((index, rejection, cause)) =
            claims
                .iter()
                .enumerate()
                .find_map(|(index, claim)| match claim.claim() {
                    RouteClaim::Rejected { rejection } => {
                        Some((index, rejection, claim.unsent_cause()))
                    }
                    _ => None,
                })
        {
            let Some(route) = self.routes.get(index) else {
                return Err(DeliveryContractError::InvalidEvidence);
            };
            return Ok(RouteDecision::Complete(Box::new(
                NativeDeliveryResult::rejected(
                    rejection.clone(),
                    cause.ok_or(DeliveryContractError::InvalidEvidence)?,
                )
                .with_reachability(route.reachability()),
            )));
        }
        if let Some(index) = claims
            .iter()
            .position(|claim| matches!(claim.claim(), RouteClaim::Holds))
        {
            return Ok(RouteDecision::Selected(index));
        }
        if let Some((detail, cause)) = claims.iter().find_map(|claim| match claim.claim() {
            RouteClaim::LiveElsewhere {
                detail: Some(detail),
                ..
            } => Some((detail.as_str(), claim.unsent_cause())),
            _ => None,
        }) {
            return Ok(RouteDecision::Complete(Box::new(
                NativeDeliveryResult::rejected(
                    DeliveryRejection {
                        reason: DeliveryRejectionReason::LiveElsewhere,
                        next_action: DeliveryNextAction::InspectTarget,
                        client_code: None,
                        detail: Some(detail.to_owned()),
                        claims: None,
                        native_failure: None,
                    },
                    cause.ok_or(DeliveryContractError::InvalidEvidence)?,
                ),
            )));
        }
        if let Some(claim) = claims
            .iter()
            .find(|claim| matches!(claim.claim(), RouteClaim::LiveElsewhere { .. }))
        {
            return Ok(RouteDecision::Complete(Box::new(
                NativeDeliveryResult::rejected(
                    DeliveryRejection {
                        reason: DeliveryRejectionReason::LiveElsewhere,
                        next_action: DeliveryNextAction::InspectTarget,
                        client_code: None,
                        detail: Some(
                            "session is live in a Claude Code process Router cannot message".into(),
                        ),
                        claims: None,
                        native_failure: None,
                    },
                    claim
                        .unsent_cause()
                        .ok_or(DeliveryContractError::InvalidEvidence)?,
                ),
            )));
        }
        if let Some(index) = claims
            .iter()
            .position(|claim| matches!(claim.claim(), RouteClaim::CanLoad))
        {
            return Ok(RouteDecision::Selected(index));
        }
        if let Some((reason, cause)) = claims.iter().find_map(|claim| match claim.claim() {
            RouteClaim::Unavailable {
                reason,
                retryable: true,
            } => Some((reason, claim.unsent_cause())),
            _ => None,
        }) {
            return Ok(RouteDecision::Complete(Box::new(
                NativeDeliveryResult::not_submitted(
                    reason.reason.clone(),
                    true,
                    cause.ok_or(DeliveryContractError::InvalidEvidence)?,
                ),
            )));
        }
        let reasons: Vec<_> = claims
            .iter()
            .filter_map(|claim| match claim.claim() {
                RouteClaim::Unavailable { reason, .. } => {
                    Some(format!("{}; {}", reason.reason, reason.fix))
                }
                _ => None,
            })
            .collect();
        let reason = if reasons.is_empty() {
            DeliveryRejectionReason::NoRoute
        } else {
            DeliveryRejectionReason::EndpointUnavailable
        };
        let cause = if reasons.is_empty() {
            UnsentCause::RecipientUnknown
        } else {
            claims
                .iter()
                .find(|claim| matches!(claim.claim(), RouteClaim::Unavailable { .. }))
                .and_then(collaboration_contracts::NativeRouteClaim::unsent_cause)
                .ok_or(DeliveryContractError::InvalidEvidence)?
        };
        Ok(RouteDecision::Complete(Box::new(
            NativeDeliveryResult::rejected(
                DeliveryRejection {
                    reason,
                    next_action: if reasons.is_empty() {
                        DeliveryNextAction::CorrectRequest
                    } else {
                        DeliveryNextAction::RetryLater
                    },
                    client_code: None,
                    detail: Some(if reasons.is_empty() {
                        "no route serves this endpoint".into()
                    } else {
                        reasons.join("; ")
                    }),
                    claims: None,
                    native_failure: None,
                },
                cause,
            ),
        )))
    }

    async fn deliver_through(
        &self,
        index: usize,
        mut request: collaboration_contracts::layer_zero::DeliveryRequest,
        evidence: &dyn AttemptEvidenceSink,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        let route = self
            .routes
            .get(index)
            .ok_or(DeliveryContractError::InvalidEvidence)?;
        // Resolve unsupported queueing before dispatch so one attempt has one native effect.
        if request.mode == MessageDelivery::Queue
            && !route.supports_delivery_mode(&request.target, MessageDelivery::Queue)
        {
            request.mode = MessageDelivery::Steer;
        }
        Ok(route
            .deliver(request, evidence)
            .await?
            .with_reachability(route.reachability()))
    }
}

impl SessionMessageDelivery for SessionDeliveryRouter {
    fn supports_delivery_mode(&self, target: &SessionRef, mode: MessageDelivery) -> bool {
        self.routes
            .iter()
            .all(|route| route.supports_delivery_mode(target, mode))
    }

    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, NativeDeliveryResult> {
        Box::pin(async move { self.deliver_once(request, evidence).await })
    }

    fn reconcile_attempt(
        &self,
        context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async move {
            let reachability = match &context.recorded {
                RouteEffectEvidence::CodexAppServer(_) => SessionReachability::CodexAppServer,
                RouteEffectEvidence::ProviderAcp(_) => SessionReachability::ProviderAcp,
                RouteEffectEvidence::ClaudeCodePeer(_) => SessionReachability::ClaudeCodePeer,
            };
            let Some(route) = self
                .routes
                .iter()
                .find(|route| route.reachability() == reachability)
            else {
                return Ok(AttemptReconciliation::StillUnknown);
            };
            route.reconcile_attempt(context).await
        })
    }
}

impl TargetPresenceProbe for SessionDeliveryRouter {
    fn presence(&self, target: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        let target = target.clone();
        Box::pin(async move {
            let presences = join_all(self.routes.iter().map(|route| route.presence(&target)))
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            Ok(aggregate_presence(&presences))
        })
    }
}

fn aggregate_presence(presences: &[RoutePresence]) -> TargetPresence {
    if presences
        .iter()
        .any(|presence| matches!(presence, RoutePresence::Running))
    {
        return TargetPresence::Running;
    }

    let has_live_elsewhere = presences
        .iter()
        .any(|presence| matches!(presence, RoutePresence::LiveElsewhere { .. }));
    let mut unreachable_reasons = presences
        .iter()
        .filter_map(|presence| match presence {
            RoutePresence::Unreachable { reason } => Some(reason.as_str()),
            _ => None,
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if has_live_elsewhere {
        let mut reasons = vec!["live elsewhere".to_owned()];
        for detail in presences.iter().filter_map(|presence| match presence {
            RoutePresence::LiveElsewhere { detail } => detail.as_deref(),
            _ => None,
        }) {
            reasons.push(detail.to_owned());
        }
        reasons.extend(unreachable_reasons);
        return TargetPresence::Unreachable {
            reason: reasons.join("; "),
        };
    }

    if presences
        .iter()
        .any(|presence| matches!(presence, RoutePresence::Wakeable))
    {
        return TargetPresence::Wakeable;
    }
    if presences
        .iter()
        .any(|presence| matches!(presence, RoutePresence::NotRunning))
    {
        return TargetPresence::NotRunning;
    }

    if unreachable_reasons.is_empty() {
        unreachable_reasons.push("no route holds or can load this session".to_owned());
    }
    TargetPresence::Unreachable {
        reason: unreachable_reasons.join("; "),
    }
}

#[cfg(test)]
#[path = "session_delivery_router_test.rs"]
mod tests;
