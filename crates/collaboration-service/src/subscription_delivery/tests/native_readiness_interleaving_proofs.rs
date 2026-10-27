//! Reconcile twice while real SDK work is Pending, before any dispatch marks Attempted.
use super::native_follow_mail_proofs::frozen_follow;
use super::*;

#[tokio::test]
async fn a_second_readiness_reconcile_while_sdk_attempt_is_pending_preserves_backoff() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let peer = PeerId::new(uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap())
        .unwrap();
    // Compose the production owners, but drive the real SDK engine explicitly.
    // Not starting its background runner deterministically keeps released work Pending.
    let mail = Arc::new(
        crate::MailDeliveryRuntime::new(
            peer,
            crate::MailPushDeliveryProps {
                interactions: fixture.interactions.clone(),
                actors: runtime.service.clone(),
                presence: fixture.presence.clone(),
                machine: collaboration_contracts::MachineIdentity::new(
                    SERVICE_ID.to_owned().try_into().unwrap(),
                    None,
                )
                .unwrap(),
                clock: fixture.clock.clone(),
            },
            None,
        )
        .unwrap(),
    );
    let publisher = Arc::new(crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&mail),
    ));
    let app = CollaborationApplication::new(
        ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_automation_store(fixture.automation.clone())
            .with_project_admission(fixture.admission.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone())
            .with_interaction_store(fixture.interactions.clone())
            .with_installation_peer(peer)
            .unwrap()
            .with_mail_delivery_runtime(mail.clone())
            .unwrap()
            .with_notification_publisher(publisher.clone()),
    );
    fixture
        .post(
            &fixture.root,
            "one frozen follow held before repeated readiness",
        )
        .await;
    let frozen = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let items = frozen_follow(&fixture).await;
            if !items.is_empty() {
                break items;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(frozen.len(), 1);
    publisher.wait_project_idle_for_test(&fixture.project).await;
    mail.drive_due_for_test().await.unwrap();
    let push = frozen[0].id().as_uuid().to_string().try_into().unwrap();
    fixture
        .await_push_state(&push, PushDeliveryState::Held)
        .await;
    let held = fixture
        .interactions
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // Each explicit pass clears the presence cache. No SDK engine is driven
    // between the two releases. Record the projection without making it the
    // early-failure oracle: the wrong build must reach the actual early native call.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let mut released = Vec::new();
    for _ in 0..2 {
        runtime
            .service
            .reconcile_reader(fixture.reader.clone())
            .await
            .unwrap();
        runtime.owner_barrier(&fixture.reader).await;
        released.push(
            fixture
                .interactions
                .get_push_record(&push)
                .await
                .unwrap()
                .unwrap()
                .delivery_state,
        );
        assert!(runtime.requests.lock().await.try_recv().is_err());
    }
    let pending_view = fixture.public_subscription_view(&runtime).await;
    let pending_show = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: target_session(&fixture.reader).unwrap(),
            reference: push.as_str().to_owned(),
        })
        .await
        .unwrap();

    let worker = mail.clone();
    let mut driving = tokio::spawn(async move { worker.drive_due_for_test().await });
    let first = tokio::time::timeout(Duration::from_secs(6), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(first.payload.push_id, push);
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "failed after the two readiness releases".into(),
        })
        .unwrap();
    let mut early = None;
    tokio::time::timeout(Duration::from_secs(6), async {
        tokio::select! {
            result = &mut driving => result.unwrap().unwrap(),
            request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                early = Some(request.payload.push_id);
                runtime.completions.send(DeliveryOutcome::Started).unwrap();
                driving.await.unwrap().unwrap();
            }
        }
    })
    .await
    .unwrap();
    let expected =
        chrono::DateTime::from_timestamp_millis(fixture.clock.now().timestamp_millis() + 30_000)
            .unwrap();
    let observed_deadline = fixture
        .public_subscription_view(&runtime)
        .await
        .next_retry_at;
    if early.is_none() {
        fixture.clock.advance_without_tokio_time(29);
        let worker = mail.clone();
        let mut driving = tokio::spawn(async move { worker.drive_due_for_test().await });
        tokio::time::timeout(Duration::from_secs(6), async {
            tokio::select! {
                result = &mut driving => result.unwrap().unwrap(),
                request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                    early = Some(request.payload.push_id);
                    runtime.completions.send(DeliveryOutcome::Started).unwrap();
                    driving.await.unwrap().unwrap();
                }
            }
        })
        .await
        .unwrap();
    }
    let mut at_deadline = None;
    if early.is_none() {
        fixture.clock.advance_without_tokio_time(1);
        let worker = mail.clone();
        let driving = tokio::spawn(async move { worker.drive_due_for_test().await });
        let request = tokio::time::timeout(Duration::from_secs(6), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .unwrap();
        at_deadline = Some(request.payload.push_id);
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        tokio::time::timeout(Duration::from_secs(6), driving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    drop(mail);
    runtime.close().await;
    let interactions = fixture.interactions.clone();
    let _directory = fixture.into_directory();
    Arc::try_unwrap(interactions)
        .unwrap_or_else(|_| panic!("readiness fixture retained native owner"))
        .close()
        .await
        .unwrap();
    assert!(
        early.is_none(),
        "a second readiness release accelerated {early:?} before its real backoff"
    );
    assert_eq!(observed_deadline, Some(expected));
    assert_eq!(at_deadline, Some(push));
    assert_eq!(released, vec![PushDeliveryState::Pending; 2]);
    assert_eq!(
        pending_show.record.delivery_state,
        PushDeliveryState::Pending
    );
    assert!(pending_show.record.last_outcome.is_none());
    assert_eq!(pending_show.record.header_facts, held.header_facts);
    assert_eq!(pending_show.record.activity, held.activity);
    assert_eq!(pending_view.pending_count, 1);
    assert!(pending_view.held_since.is_some());
    assert!(pending_view.next_retry_at.is_none());
}
