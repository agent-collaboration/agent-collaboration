//! Real readiness triggers for native held custody, with only the external client scripted.
//!
//! Held mail is checked when the reader first sees it and again only after a signal that
//! its target may have loaded: a Codex thread load recorded in the lifecycle journal, a
//! provider supervisor load, or an observation retry. There is no presence timer.
use super::super::super::subscription_service::OwnerObservation;
use super::*;
/// Sends one queued message to the reader's own session, which the fixture holds.
async fn send_held(
    app: &CollaborationApplication,
    target: &collaboration_protocol::SessionRef,
    text: &str,
) -> (
    SessionMessageSendParams,
    collaboration_protocol::PushMessageSendResult,
) {
    let input = SessionMessageSendParams {
        item_id: Some(MailItemId::generate()),
        target: target.clone(),
        message: MessageContent::HumanUser {
            text: text.to_owned().try_into().unwrap(),
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
    (input, held)
}

#[tokio::test]
async fn held_native_mail_waits_for_a_load_signal_and_holds_no_presence_timer() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "hold until this reader loads").await;
    // Actual SDK-held custody must never become a carrier recovery candidate.
    assert!(
        native
            .interactions
            .direct_message_recovery_targets()
            .await
            .unwrap()
            .is_empty()
    );
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(|event| matches!(event,
        OwnerObservation::NativeHeldChecked { item, running: false } if item == &held.push_id
    ))).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(None))),
    )
    .await
    .unwrap();
    fixture.assert_no_legacy_message_tables().await;

    // An hour with no signal checks nothing.
    let checks_while_held = fixture.presence.checks();
    fixture.clock.advance_without_tokio_time(3_600);
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.owner_barrier(&fixture.reader),
    )
    .await
    .unwrap();
    assert_eq!(
        fixture.presence.checks(),
        checks_while_held,
        "no presence timer"
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // An observation retry is one more check; the target is still not running.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Unreachable {
        reason: "fixture reader is unreachable".into(),
    };
    fixture.held_mail.observation_retried();
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(|event| matches!(event,
        OwnerObservation::NativeHeldChecked { item, running: false } if item == &held.push_id
    ))).await.unwrap();
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

    // A provider load is a signal too; this time the target runs, and the mail goes.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.held_mail.provider_completed();
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

/// A native observation of the reader's own thread going idle, as the observer records
/// a thread that loaded.
fn thread_loaded(
    target: &collaboration_protocol::SessionRef,
) -> collaboration_protocol::LifecycleObservation {
    serde_json::from_value(serde_json::json!({
        "observedAt":"2027-01-15T08:01:00.000Z","source":"nativeNotification",
        "scope":{"endpoint":target.endpoint,
            "generation":{"serviceEpoch":SERVICE_ID,"generation":1},
            "observerId":"00000000-0000-4000-8000-000000000003"},
        "subject":{"kind":"thread","address":{"endpoint":target.endpoint,"nativeThreadId":target.session_id}},
        "change":{"kind":"threadStatus","status":{"type":"idle"},"ordering":"established"},
    }))
    .unwrap()
}

#[tokio::test]
async fn a_codex_thread_that_loads_during_the_check_still_releases_its_held_mail() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let journal = Arc::new(lifecycle_observation::LifecycleStore::new(
        lifecycle_observation::ObservationJournal::open(
            &fixture.directory.path().join("session-registry.sqlite"),
            SERVICE_ID.to_owned().try_into().unwrap(),
        )
        .await
        .unwrap(),
    ));
    journal
        .prepare(chrono::Utc::now().timestamp())
        .await
        .unwrap();
    *fixture.held_mail.journal.lock().unwrap() = Some(Arc::clone(&journal));
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (read, read_signal) = tokio::sync::oneshot::channel();
    let (answer_signal, answer) = tokio::sync::oneshot::channel();

    // The reader's check reads "wakeable" and is paused before it answers. Meanwhile the
    // thread loads: the journal records it, and the target now runs. The check still
    // answers what it read, so the mail is held; the load it missed owes another check.
    let (input, held) = send_held(&app, &target, "release on the load the check missed").await;
    *fixture.presence.2.lock().unwrap() = Some(PresencePause { read, answer });
    fixture.held_mail.observation_retried();
    tokio::time::timeout(Duration::from_secs(10), read_signal)
        .await
        .unwrap()
        .unwrap();
    journal
        .append(&thread_loaded(&target), chrono::Utc::now().timestamp())
        .await
        .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    answer_signal.send(()).unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("the load recorded during the check owed another check");
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference_for_request(&input, &held.push_id, peer)).await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn explicit_reader_reconcile_releases_native_held_mail_before_the_next_presence_tick() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
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

#[tokio::test]
async fn a_journal_record_for_another_thread_checks_nothing_and_its_own_load_releases() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let journal = Arc::new(lifecycle_observation::LifecycleStore::new(
        lifecycle_observation::ObservationJournal::open(
            &fixture.directory.path().join("session-registry.sqlite"),
            SERVICE_ID.to_owned().try_into().unwrap(),
        )
        .await
        .unwrap(),
    ));
    journal
        .prepare(chrono::Utc::now().timestamp())
        .await
        .unwrap();
    *fixture.held_mail.journal.lock().unwrap() = Some(Arc::clone(&journal));
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "release only on this thread's load").await;
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(|event| matches!(event,
        OwnerObservation::NativeHeldChecked { item, running: false } if item == &held.push_id
    ))).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.owner_barrier(&fixture.reader),
    )
    .await
    .unwrap();

    // Another thread on the same endpoint loads: the journal commits, nothing is owed.
    let checks_while_held = fixture.presence.checks();
    let mut other = target.clone();
    other.session_id = "another-thread".to_owned().try_into().unwrap();
    journal
        .append(&thread_loaded(&other), chrono::Utc::now().timestamp())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime.observe(|event| {
            matches!(event, OwnerObservation::HeldMailSignalTaken { owed: false })
        }),
    )
    .await
    .expect("the reader took the journal commit and owed nothing");
    assert_eq!(
        fixture.presence.checks(),
        checks_while_held,
        "a load of another thread owes this reader nothing"
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // Its own thread loads: one check, and the mail goes.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    journal
        .append(&thread_loaded(&target), chrono::Utc::now().timestamp())
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("its own thread's load owed a check");
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference_for_request(&input, &held.push_id, peer)).await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
