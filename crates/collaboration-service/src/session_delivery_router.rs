//! Selects one injected client route for an attempt, then keeps that choice fixed.
use crate::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext,
    DeliveryContractError, DeliveryFuture, NativeDeliveryResult, RouteClaim, RoutePresence,
    SessionDeliveryRoute, SessionMessageDelivery, TargetPresence, TargetPresenceProbe, UnsentCause,
};
use agent_automation::RouteEffectEvidence;
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
        request: crate::layer_zero::DeliveryRequest,
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
                .and_then(crate::NativeRouteClaim::unsent_cause)
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
        request: crate::layer_zero::DeliveryRequest,
        evidence: &dyn AttemptEvidenceSink,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        let route = self
            .routes
            .get(index)
            .ok_or(DeliveryContractError::InvalidEvidence)?;
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
        request: crate::layer_zero::DeliveryRequest,
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

    if unreachable_reasons.is_empty() {
        unreachable_reasons.push("no route holds or can load this session".to_owned());
    }
    TargetPresence::Unreachable {
        reason: unreachable_reasons.join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::{RouteDecision, SessionDeliveryRouter};
    use crate::{
        AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
        DeliveryPrecondition, DeliveryReceipt, LoadPolicy, RoutePresence, SessionDeliveryRoute,
        SessionMessageDelivery,
    };
    use agent_automation::AttemptId;
    use collaboration_protocol::{
        DeliveryCorrelationId, DeliveryNextAction, DeliveryOutcome, DeliveryPeerClaim,
        DeliveryRejection, DeliveryRejectionReason, MessageDelivery, MessageText, PushId,
        SessionReachability, SessionRef,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct CapturingRoute {
        requests: Mutex<Vec<crate::layer_zero::DeliveryRequest>>,
        claim: Option<crate::NativeRouteClaim>,
    }

    impl SessionDeliveryRoute for CapturingRoute {
        fn reachability(&self) -> SessionReachability {
            SessionReachability::ClaudeCodePeer
        }

        fn claim(&self, _: &SessionRef) -> DeliveryFuture<'_, crate::NativeRouteClaim> {
            let claim = self
                .claim
                .clone()
                .unwrap_or_else(crate::NativeRouteClaim::holds);
            Box::pin(async move { Ok(claim) })
        }

        fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
            Box::pin(async { Ok(RoutePresence::Running) })
        }

        fn deliver<'a>(
            &'a self,
            request: crate::layer_zero::DeliveryRequest,
            _: &'a dyn AttemptEvidenceSink,
        ) -> DeliveryFuture<'a, crate::NativeDeliveryResult> {
            self.requests
                .lock()
                .expect("captured requests")
                .push(request);
            Box::pin(async {
                Ok(crate::native_delivery_result::scripted_delivery_result(
                    DeliveryReceipt {
                        outcome: DeliveryOutcome::Started,
                        reachability: None,
                        client: None,
                    },
                ))
            })
        }

        fn reconcile_attempt(
            &self,
            _: AttemptReconciliationContext,
        ) -> DeliveryFuture<'_, AttemptReconciliation> {
            Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
        }
    }

    #[tokio::test]
    async fn typed_route_rejection_precedes_another_routes_holds_claim()
    -> Result<(), Box<dyn std::error::Error>> {
        let rejection = DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::LiveElsewhere,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: None,
            detail: Some("two live terminals claim this session".to_owned()),
            claims: Some(vec![DeliveryPeerClaim {
                pid: 52304,
                name: Some("terminal-one".to_owned()),
                cwd: Some("/workspace/one".to_owned()),
            }]),
        };
        let holds = Arc::new(CapturingRoute {
            claim: Some(crate::NativeRouteClaim::holds()),
            ..CapturingRoute::default()
        });
        let rejected = Arc::new(CapturingRoute {
            claim: Some(crate::NativeRouteClaim::rejected(
                rejection,
                crate::UnsentCause::for_rejection(DeliveryRejectionReason::LiveElsewhere),
            )),
            ..CapturingRoute::default()
        });
        let router = SessionDeliveryRouter::new(vec![holds, rejected]);
        let target: SessionRef = serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"target"
        }))
        .expect("target session");

        let decision = router.select_route(&target).await.expect("route selection");

        let RouteDecision::Complete(receipt) = decision else {
            return Err("typed rejection did not stop route selection".into());
        };
        if receipt.receipt().reachability != Some(SessionReachability::ClaudeCodePeer)
            || !matches!(
            receipt.receipt().outcome.clone(),
            DeliveryOutcome::Rejected(rejection)
                if rejection.claims.as_ref().is_some_and(|claims| claims.len() == 1)
            )
        {
            return Err("typed rejection lost route reachability or claims".into());
        }
        Ok(())
    }

    #[tokio::test]
    async fn prepared_push_uses_the_selected_route_without_changing_its_line_or_policy() {
        let push_id = PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
            .expect("UUIDv7 push id");
        let expected_push_id = push_id.clone();
        let link = format!(
            "router://00000000-0000-4000-8000-000000000001/push/{}",
            push_id.as_str()
        );
        let line =
            MessageText::try_from(format!("✉️ sender · \"preview\" · {link}")).expect("push line");
        let target: SessionRef = serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"target"
        }))
        .expect("target session");
        let correlation = DeliveryCorrelationId::try_from(push_id.as_str().to_owned())
            .expect("push id correlation");
        let route = Arc::new(CapturingRoute::default());
        let router = SessionDeliveryRouter::new(vec![route.clone()]);

        let receipt = router
            .deliver(
                crate::layer_zero::DeliveryRequest {
                    payload: crate::layer_zero::PreparedPush {
                        push_id,
                        line: line.clone(),
                        load_policy: LoadPolicy::LoadedOnly,
                    },
                    target: target.clone(),
                    mode: MessageDelivery::Queue,
                    precondition: DeliveryPrecondition::Unpinned,
                    correlation: correlation.clone(),
                    attempt: AttemptId::generate(),
                },
                &crate::session_delivery_contract::UnstoredAttemptEvidenceSink,
            )
            .await
            .expect("prepared push delivery");

        assert_eq!(receipt.receipt().outcome.clone(), DeliveryOutcome::Started);
        assert_eq!(
            receipt.receipt().reachability,
            Some(SessionReachability::ClaudeCodePeer)
        );
        let requests = route.requests.lock().expect("captured requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].payload.push_id, expected_push_id);
        assert_eq!(requests[0].payload.line.as_str(), line.as_str());
        assert_eq!(requests[0].payload.load_policy, LoadPolicy::LoadedOnly);
        assert_eq!(requests[0].target, target);
        assert_eq!(requests[0].mode, MessageDelivery::Queue);
        assert_eq!(requests[0].correlation, correlation);
        assert!(matches!(
            &requests[0].precondition,
            DeliveryPrecondition::Unpinned
        ));
    }
}
