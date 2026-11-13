use super::{RouteDecision, SessionDeliveryRouter};
use agent_automation::AttemptId;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_protocol::DeliveryReceipt;
use collaboration_protocol::{
    DeliveryCorrelationId, DeliveryNextAction, DeliveryOutcome, DeliveryPeerClaim,
    DeliveryRejection, DeliveryRejectionReason, MessageDelivery, MessageText, PushId,
    SessionReachability, SessionRef,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CapturingRoute {
    requests: Mutex<Vec<collaboration_contracts::layer_zero::DeliveryRequest>>,
    claim: Option<collaboration_contracts::NativeRouteClaim>,
    queue_unsupported: bool,
    reachability: Option<SessionReachability>,
}

impl SessionDeliveryRoute for CapturingRoute {
    fn reachability(&self) -> SessionReachability {
        self.reachability
            .unwrap_or(SessionReachability::ClaudeCodePeer)
    }

    fn supports_delivery_mode(&self, _: &SessionRef, mode: MessageDelivery) -> bool {
        mode != MessageDelivery::Queue || !self.queue_unsupported
    }

    fn claim(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::NativeRouteClaim> {
        let claim = self
            .claim
            .clone()
            .unwrap_or_else(collaboration_contracts::NativeRouteClaim::holds);
        Box::pin(async move { Ok(claim) })
    }

    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
        Box::pin(async { Ok(RoutePresence::Running) })
    }

    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        self.requests
            .lock()
            .expect("captured requests")
            .push(request);
        Box::pin(async {
            Ok(collaboration_contracts::scripted_delivery_result(
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
        claim: Some(collaboration_contracts::NativeRouteClaim::holds()),
        ..CapturingRoute::default()
    });
    let rejected = Arc::new(CapturingRoute {
        claim: Some(collaboration_contracts::NativeRouteClaim::rejected(
            rejection,
            collaboration_contracts::UnsentCause::for_rejection(
                DeliveryRejectionReason::LiveElsewhere,
            ),
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
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push id correlation");
    let route = Arc::new(CapturingRoute::default());
    let unrelated_route = Arc::new(CapturingRoute {
        claim: Some(collaboration_contracts::NativeRouteClaim::not_mine()),
        queue_unsupported: true,
        ..CapturingRoute::default()
    });
    let router = SessionDeliveryRouter::new(vec![unrelated_route, route.clone()]);

    let receipt = router
        .deliver(
            collaboration_contracts::layer_zero::DeliveryRequest {
                payload: collaboration_contracts::layer_zero::PreparedPush {
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
            &collaboration_contracts::UnstoredAttemptEvidenceSink,
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

#[tokio::test]
async fn unsupported_queue_steers_once_for_every_route_kind() {
    for reachability in [
        SessionReachability::CodexAppServer,
        SessionReachability::ProviderAcp,
        SessionReachability::ClaudeCodePeer,
    ] {
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
        let route = Arc::new(CapturingRoute {
            queue_unsupported: true,
            reachability: Some(reachability),
            ..CapturingRoute::default()
        });
        let router = SessionDeliveryRouter::new(vec![route.clone()]);

        let receipt = router
            .deliver(
                collaboration_contracts::layer_zero::DeliveryRequest {
                    payload: collaboration_contracts::layer_zero::PreparedPush {
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
                &collaboration_contracts::UnstoredAttemptEvidenceSink,
            )
            .await
            .expect("prepared push delivery");

        assert_eq!(receipt.receipt().outcome.clone(), DeliveryOutcome::Started);
        assert_eq!(receipt.receipt().reachability, Some(reachability));
        let requests = route.requests.lock().expect("captured requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].payload.push_id, expected_push_id);
        assert_eq!(requests[0].payload.line.as_str(), line.as_str());
        assert_eq!(requests[0].payload.load_policy, LoadPolicy::LoadedOnly);
        assert_eq!(requests[0].target, target);
        assert_eq!(requests[0].mode, MessageDelivery::Steer);
        assert_eq!(requests[0].correlation, correlation);
        assert!(matches!(
            &requests[0].precondition,
            DeliveryPrecondition::Unpinned
        ));
    }
}
