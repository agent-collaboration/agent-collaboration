use super::super::{
    SubscriptionClock, SubscriptionWaitFilter, subscription_service::OwnerObservation,
};
use super::owner_fixture::*;
use collaboration_contracts::AttemptReconciliation;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, SessionReachability,
};
use message_board::{SubscriptionDeliveryOutcome, SubscriptionMode, SubscriptionScope, WhenIdle};

#[tokio::test(start_paused = true)]
async fn declared_renewal_before_covering_wait_changes_only_the_covered_membership() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let uncovered_root = fixture.add_root().await;
    for root in [&fixture.root, &uncovered_root] {
        fixture
            .policy(root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
            .await;
    }
    let covered_scope = SubscriptionScope::thread(fixture.root.clone());
    let uncovered_scope = SubscriptionScope::thread(uncovered_root);
    let (covered_before, uncovered_before) = {
        let store = fixture.store.lock().await;
        (
            store
                .get_thread_subscription_record(
                    &fixture.reader,
                    &covered_scope,
                    fixture.clock.now(),
                )
                .await
                .unwrap()
                .unwrap(),
            store
                .get_thread_subscription_record(
                    &fixture.reader,
                    &uncovered_scope,
                    fixture.clock.now(),
                )
                .await
                .unwrap()
                .unwrap(),
        )
    };
    let runtime = fixture.runtime().await;
    fixture.clock.advance(3600).await;
    runtime.owner_barrier(&fixture.reader).await;
    let renewal_time = fixture.clock.now();
    let non_covering = runtime
        .service
        .wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::Roots(vec![message_board::ThreadRef {
                thread: message_board::ThreadId::generate(),
                project: Some(fixture.project.clone()),
            }]),
            0,
            usize::MAX,
        )
        .await
        .unwrap_err();
    assert_eq!(
        non_covering.kind,
        message_board::BoardFailureKind::InvalidField
    );
    for (scope, before) in [
        (&covered_scope, &covered_before),
        (&uncovered_scope, &uncovered_before),
    ] {
        let unchanged = fixture
            .store
            .lock()
            .await
            .get_thread_subscription_record(&fixture.reader, scope, fixture.clock.now())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.renewed_at(), before.renewed_at());
        assert_eq!(unchanged.expires_at(), before.expires_at());
    }
    fixture
        .write_as(
            fixture.reader.clone(),
            message_board::BoardOperation::ThreadRenew {
                scope: covered_scope.clone(),
            },
            renewal_time,
        )
        .await;
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let covered_root = fixture.root.clone();
    let waiter = tokio::spawn(async move {
        service
            .wait(
                reader,
                SubscriptionWaitFilter::Roots(vec![(covered_root).into()]),
                60,
                usize::MAX,
            )
            .await
            .unwrap()
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
    let (covered_after, uncovered_after) = {
        let store = fixture.store.lock().await;
        (
            store
                .get_thread_subscription_record(
                    &fixture.reader,
                    &covered_scope,
                    fixture.clock.now(),
                )
                .await
                .unwrap()
                .unwrap(),
            store
                .get_thread_subscription_record(
                    &fixture.reader,
                    &uncovered_scope,
                    fixture.clock.now(),
                )
                .await
                .unwrap()
                .unwrap(),
        )
    };
    assert_eq!(covered_after.renewed_at(), renewal_time);
    assert_eq!(
        covered_after.expires_at(),
        renewal_time + (covered_before.expires_at() - covered_before.renewed_at())
    );
    assert!(covered_after.renewed_at() > covered_before.renewed_at());
    assert!(covered_after.expires_at() > covered_before.expires_at());
    assert_eq!(uncovered_after.renewed_at(), uncovered_before.renewed_at());
    assert_eq!(uncovered_after.expires_at(), uncovered_before.expires_at());
    fixture.clock.advance(60).await;
    assert!(waiter.await.unwrap().is_none());
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn loaded_only_acp_queue_is_reconciled_before_any_settlement() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.queued_runtime().await;
    fixture.post(&fixture.root, "Queued activity").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    runtime.completions.send(DeliveryOutcome::Queued).unwrap();
    let (context, reply) = runtime.reconciliations.lock().await.recv().await.unwrap();
    assert_eq!(context.target, request.target);
    let agent_automation::RouteEffectEvidence::ProviderAcp(evidence) = context.recorded else {
        panic!("captured ACP evidence");
    };
    assert_eq!(evidence.attempt_id, request.attempt);
    assert_eq!(
        evidence.submission,
        agent_automation::SubmissionEffect::RouterQueued
    );
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.delivery_state, PushDeliveryState::Attempted);
    let windows = fixture.store.lock().await;
    let mut rows = windows
        .list_reader_subscriptions(&fixture.reader, fixture.clock.now())
        .await
        .unwrap();
    windows
        .observe_subscription_deliveries(&mut rows)
        .await
        .unwrap();
    assert_eq!(rows.records.len(), 1);
    assert_eq!(
        rows.records[0].delivery.allocated_pending, 1,
        "the actual queued callback must keep its frozen allocation pending"
    );
    drop(windows);
    reply
        .send(AttemptReconciliation::Accepted(Box::new(DeliveryReceipt {
            outcome: DeliveryOutcome::Started,
            reachability: Some(SessionReachability::ProviderAcp),
            client: None,
        })))
        .ok()
        .unwrap();
    fixture
        .await_push_state(&request.payload.push_id, PushDeliveryState::Delivered)
        .await;
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.last_outcome.unwrap().outcome,
        DeliveryOutcome::Started
    );
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn acp_queue_known_not_submitted_returns_to_hold_without_advancing() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.queued_runtime().await;
    fixture.post(&fixture.root, "Not submitted").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    runtime.completions.send(DeliveryOutcome::Queued).unwrap();
    let (_, reply) = runtime.reconciliations.lock().await.recv().await.unwrap();
    // Keep the reader unavailable while asserting its readiness Hold. A Running
    // reader is allowed to release this unscheduled hold immediately (925/941).
    *fixture.presence.0.lock().unwrap() = collaboration_contracts::TargetPresence::Wakeable;
    reply
        .send(AttemptReconciliation::KnownNotSubmitted)
        .ok()
        .unwrap();
    fixture
        .await_push_state(&request.payload.push_id, PushDeliveryState::Held)
        .await;
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.delivery_state, PushDeliveryState::Held);
    let board = fixture.public_subscription_view(&runtime).await;
    assert_eq!(board.pending_count, 1);
    assert_eq!(board.held_since, Some(fixture.clock.now()));
    assert!(runtime.requests.lock().await.try_recv().is_err());
    *fixture.presence.0.lock().unwrap() = collaboration_contracts::TargetPresence::Running;
    fixture.clock.advance(30).await;
    let retried = runtime.requests.lock().await.recv().await.unwrap();
    assert_eq!(retried.payload.push_id, request.payload.push_id);
    runtime.await_settled().await;
    assert_eq!(
        fixture
            .public_subscription_view(&runtime)
            .await
            .pending_count,
        0
    );
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn acp_queue_still_unknown_settles_unknown_with_route_evidence() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.queued_runtime().await;
    fixture.post(&fixture.root, "Unknown queue").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    runtime.completions.send(DeliveryOutcome::Queued).unwrap();
    let (_, reply) = runtime.reconciliations.lock().await.recv().await.unwrap();
    reply
        .send(AttemptReconciliation::StillUnknown)
        .ok()
        .unwrap();
    fixture
        .await_push_state(&request.payload.push_id, PushDeliveryState::OutcomeUnknown)
        .await;
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.delivery_state, PushDeliveryState::OutcomeUnknown);
    let board = fixture.public_subscription_view(&runtime).await;
    let Some(SubscriptionDeliveryOutcome::Unknown { evidence }) = board.last_outcome else {
        panic!("unknown board evidence");
    };
    runtime.close().await;
    let fixture = fixture.reopen_messages().await;
    let runtime = fixture.native_runtime().await;
    let reopened = fixture.public_subscription_view(&runtime).await;
    let Some(SubscriptionDeliveryOutcome::Unknown {
        evidence: recovered,
    }) = reopened.last_outcome
    else {
        panic!("unknown evidence after physical owner reopen");
    };
    assert_eq!(recovered, evidence);
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
    assert_eq!(evidence["routeEvidence"]["submission"], "routerQueued");
    assert_eq!(
        evidence["routeEvidence"]["attemptId"],
        request.attempt.as_str()
    );
    assert_eq!(
        evidence["routeEvidence"]["target"],
        serde_json::to_value(&request.target).unwrap()
    );
}
