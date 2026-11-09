//! Real readiness triggers for native held custody, with only the external client scripted.
//!
//! Held mail is checked when the reader first sees it and again only after a signal that
//! its target may have loaded: a Codex thread load recorded in the lifecycle journal, a
//! provider supervisor load, or an observation retry. There is no presence timer.
use super::super::super::subscription_service::OwnerObservation;
use super::*;
/// Sends one queued message to the reader's own session, which the fixture holds.
pub(super) async fn send_held(
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

/// Bounds a wait for an event the owner causes; it only stops a hang.
const HELD_CHECK_EVENT: Duration = Duration::from_secs(10);

/// A reader checks held `item` while its target is not running and waits on its signals,
/// holding it; then the target loads. With no presence timer, only that load signal checks
/// the item again, so a test that releases held mail must send it.
///
/// The signal goes only once a reader holds the item and waits. A reader started later,
/// as when a membership end replaces the owner, checks the item itself and so owes nothing
/// for a signal sent before it existed; that check then finds the target running.
pub(super) async fn checked_while_wakeable_then_loaded(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
    item: &collaboration_protocol::PushId,
) {
    tokio::time::timeout(
        HELD_CHECK_EVENT,
        runtime.observe(|event| {
            matches!(event, OwnerObservation::NativeHeldChecked { item: checked, running: false } if checked == item)
        }),
    )
    .await
    .expect("the reader checked the held item while its target was not running");
    tokio::time::timeout(
        HELD_CHECK_EVENT,
        runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(_))),
    )
    .await
    .expect("the reader holding the item waited on its signals");
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.held_mail.observation_retried();
}

/// A native observation of the reader's own thread going idle, as the observer records
/// a thread that loaded.
pub(super) fn thread_loaded(
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
    let journal = fixture.attach_journal().await;
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
    let journal = fixture.attach_journal().await;
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

#[tokio::test]
async fn held_mail_whose_target_is_not_running_goes_back_through_delivery_instead_of_staying_held()
{
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "held, then its terminal is gone").await;
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(|event| matches!(event,
        OwnerObservation::NativeHeldChecked { item, running: false } if item == &held.push_id
    ))).await.unwrap();

    // The next check finds a target that is not running and that nothing can start. Its
    // mail is not kept held for a load that will never come: it goes back through delivery,
    // whose route refuses it.
    *fixture.presence.0.lock().unwrap() = TargetPresence::NotRunning;
    fixture.held_mail.observation_retried();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("mail held for a target that is not running went back to delivery");
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference_for_request(&input, &held.push_id, peer)).await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

/// The refusal a Claude Code peer route answers for a terminal that is not running.
fn peer_not_running_refusal() -> DeliveryOutcome {
    DeliveryOutcome::Rejected(collaboration_protocol::DeliveryRejection {
        reason: collaboration_protocol::DeliveryRejectionReason::EndpointUnavailable,
        next_action: collaboration_protocol::DeliveryNextAction::RetryLater,
        client_code: None,
        detail: Some(
            "target is not running; open the Claude Code session, then send again".to_owned(),
        ),
        claims: None,
        native_failure: None,
    })
}

#[tokio::test]
async fn mail_held_when_the_runtime_reopens_for_a_target_not_running_is_refused_on_the_first_check()
{
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let initial = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &initial, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (_, held) = send_held(&app, &target, "held before the terminal closed").await;
    assert!(initial.requests.lock().await.try_recv().is_err());
    drop(app);
    initial.close().await;

    // The runtime reopens with the mail still held and the terminal gone. Only the
    // reader's first check runs: no load signal, no observation retry, no clock move.
    let fixture = fixture.reopen_native(native).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::NotRunning;
    let clock_at_reopen = fixture.clock.now();
    let restarted = fixture.runtime_without_board().await;
    let (app, native) = native_application(&fixture, &restarted, peer);
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        restarted.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("the first check sent the held mail back to delivery");
    assert_eq!(request.payload.push_id, held.push_id);

    // The peer route refuses it, and the refusal is final.
    restarted
        .completions
        .send(peer_not_running_refusal())
        .unwrap();
    await_native_completion(&restarted, 1).await;
    let settled = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = native
                .interactions
                .get_push_record(&held.push_id)
                .await
                .unwrap()
                .unwrap();
            if record.delivery_state != PushDeliveryState::Held
                && record.delivery_state != PushDeliveryState::Attempted
                && record.delivery_state != PushDeliveryState::Pending
            {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the refused mail settled");
    // With nothing left to hold, the reader's owner has no more work and stops.
    tokio::time::timeout(
        Duration::from_secs(10),
        restarted.observe(|event| matches!(event, OwnerObservation::Stopped)),
    )
    .await
    .expect("the reader's owner stopped with nothing left to hold");

    assert_eq!(
        settled.delivery_state,
        PushDeliveryState::Rejected,
        "refused, not held again"
    );
    assert!(
        native
            .interactions
            .list_held_mail_push_records(&target)
            .await
            .unwrap()
            .is_empty(),
        "nothing stays held"
    );
    assert!(
        restarted.requests.lock().await.try_recv().is_err(),
        "exactly one delivery request"
    );
    assert_eq!(fixture.clock.now(), clock_at_reopen, "no clock move");
    drop(app);
    restarted.close().await;
    fixture.close_native(native).await;
}
