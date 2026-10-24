//! Real readiness triggers for native held custody, with only the external client scripted.
use super::super::super::subscription_service::OwnerObservation;
use super::*;

#[tokio::test]
async fn held_native_mail_uses_the_reader_presence_cadence_without_reinjecting_blocked_input() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer).await;
    let target = target_session(&fixture.reader).unwrap();
    let input = SessionMessageSendParams {
        item_id: Some(MailItemId::generate()),
        target: target.clone(),
        message: MessageContent::HumanUser {
            text: "hold until this reader actually runs"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        mode: MessageDelivery::Queue,
        generation_guard: None,
    };
    let collaboration_protocol::MessageSendResult::Local(held) = tokio::time::timeout(
        Duration::from_secs(10),
        app.messages().message_send(input.clone()),
    )
    .await
    .expect("loaded-only queue did not enter held custody")
    .unwrap() else {
        panic!("held local message returned remote custody");
    };
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    // Actual SDK-held custody must never become a carrier recovery candidate.
    assert!(
        native
            .interactions
            .direct_message_recovery_targets()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        native
            .interactions
            .list_unsent_direct_messages(&target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        native
            .interactions
            .list_held_carrier_push_records(&target)
            .await
            .unwrap()
            .is_empty()
    );

    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(Some(_)))),
    )
    .await
    .unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.assert_no_legacy_message_tables().await;
    // The existing clock wakes the existing actor; no fixture calls release_held.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Unreachable {
        reason: "fixture reader is unreachable".into(),
    };
    fixture.clock.advance_without_tokio_time(30);
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(Some(_)))),
    )
    .await
    .unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        native
            .interactions
            .get_push_record(&held.push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference_for_request(&input, &held.push_id, peer)).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn explicit_reader_reconcile_releases_native_held_mail_before_the_next_presence_tick() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer).await;
    let target = target_session(&fixture.reader).unwrap();
    let input = SessionMessageSendParams {
        item_id: Some(MailItemId::generate()),
        target: target.clone(),
        message: MessageContent::HumanUser {
            text: "reader reconcile must not wait for the next tick"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        mode: MessageDelivery::Queue,
        generation_guard: None,
    };
    let collaboration_protocol::MessageSendResult::Local(held) = tokio::time::timeout(
        Duration::from_secs(10),
        app.messages().message_send(input.clone()),
    )
    .await
    .expect("loaded-only queue did not enter held custody")
    .unwrap() else {
        panic!("held local message returned remote custody");
    };
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    // Startup may already have emitted Sleeping. Wait for this held item's
    // actual unloaded readiness check before changing its presence.
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(|event| matches!(event,
        OwnerObservation::NativeHeldChecked { item, running: false } if item == &held.push_id
    ))).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.owner_barrier(&fixture.reader),
    )
    .await
    .unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_err());
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.service.reconcile_reader(fixture.reader.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference_for_request(&input, &held.push_id, peer)).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
