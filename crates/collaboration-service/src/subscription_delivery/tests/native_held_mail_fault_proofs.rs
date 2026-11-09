//! Held native mail survives each failure on the way to its next check.
//!
//! A held item is marked checked only once its presence check, its journal position and
//! any release all succeed. A failure fails the pass, and its backoff checks again; a
//! journal position that cannot be read from owes a check that records a fresh one.
use super::super::super::subscription_service::OwnerObservation;
use super::native_held_mail_proofs::{send_held, thread_loaded};
use super::*;
use crate::native_worker_faults;
use collaboration_protocol::PushId;

/// The first failed-pass backoff step, in seconds.
const FIRST_RETRY_SECONDS: u64 = 1;

async fn observed(
    runtime: &OwnerRuntime,
    _reason: &str,
    matches: impl Fn(&OwnerObservation) -> bool,
) {
    (runtime.observe(matches)).await;
}

async fn checked_while_not_running(runtime: &OwnerRuntime, push_id: &PushId) {
    observed(runtime, "a check that found the target not running", |event| {
        matches!(event, OwnerObservation::NativeHeldChecked { item, running: false } if item == push_id)
    })
    .await;
}

async fn asleep_with_nothing_due(runtime: &OwnerRuntime) {
    observed(runtime, "a sleep with nothing due", |event| {
        matches!(event, OwnerObservation::Sleeping(None))
    })
    .await;
}

async fn direct_messages_pass_failed(runtime: &OwnerRuntime) {
    observed(runtime, "a failed direct-message pass", |event| {
        matches!(event, OwnerObservation::PassFailed("direct_messages"))
    })
    .await;
}

/// Reads the owner's events up to its next sleep and fails if any pass failed on the way.
async fn asleep_with_no_failed_pass(runtime: &OwnerRuntime) {
    observed(runtime, "a sleep with no failed pass before it", |event| {
        assert!(
            !matches!(event, OwnerObservation::PassFailed(_)),
            "a pass failed: {event:?}"
        );
        matches!(event, OwnerObservation::Sleeping(_))
    })
    .await;
}

async fn next_request(
    runtime: &OwnerRuntime,
    _reason: &str,
) -> collaboration_contracts::layer_zero::DeliveryRequest {
    (async { runtime.requests.lock().await.recv().await.unwrap() }).await
}

async fn delivery_state(native: &NativeMailOwner, push_id: &PushId) -> PushDeliveryState {
    native
        .interactions
        .get_push_record(push_id)
        .await
        .unwrap()
        .unwrap()
        .delivery_state
}

#[tokio::test]
async fn an_expired_journal_position_owes_one_check_which_records_a_fresh_position() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let journal = fixture.attach_journal().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "held past its journal history").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;

    // A check records its position and pauses; meanwhile real retention maintenance
    // expires the history after that position.
    let (read, read_signal) = tokio::sync::oneshot::channel();
    let (answer_signal, answer) = tokio::sync::oneshot::channel();
    *fixture.presence.2.lock().unwrap() = Some(PresencePause { read, answer });
    fixture.held_mail.observation_retried();
    (read_signal).await.unwrap();
    let mut other = target.clone();
    other.session_id = "another-thread".to_owned().try_into().unwrap();
    let now = fixture.clock.utc_now().timestamp();
    journal.append(&thread_loaded(&other), now).await.unwrap();
    journal.maintain(now + 90 * 86_400).await.unwrap();
    // Counted while the check is paused, so a later check the owner runs before this test
    // reads its events is not already in the count.
    let checks_before = fixture.presence.checks();
    answer_signal.send(()).unwrap();
    checked_while_not_running(&runtime, &held.push_id).await;

    // The reader cannot read after its position, so it owes one check.
    observed(&runtime, "a signal that owed a check", |event| {
        matches!(event, OwnerObservation::HeldMailSignalTaken { owed: true })
    })
    .await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;
    assert_eq!(fixture.presence.checks(), checks_before + 1);

    // That check recorded a fresh position: another thread's record owes nothing.
    journal
        .append(&thread_loaded(&other), fixture.clock.utc_now().timestamp())
        .await
        .unwrap();
    observed(&runtime, "a signal that owed nothing", |event| {
        matches!(event, OwnerObservation::HeldMailSignalTaken { owed: false })
    })
    .await;
    assert_eq!(fixture.presence.checks(), checks_before + 1);
    assert_eq!(
        delivery_state(&native, &held.push_id).await,
        PushDeliveryState::Held
    );

    // Its own thread loads after the fresh position: the mail goes.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    journal
        .append(&thread_loaded(&target), fixture.clock.utc_now().timestamp())
        .await
        .unwrap();
    let request = next_request(&runtime, "its own load after the fresh position").await;
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn a_failed_position_capture_fails_the_pass_and_the_retry_keeps_the_mail_held() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let journal = fixture.attach_journal().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "held across a failed capture").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;

    // The next check cannot record where the journal is: it may not keep the mail held
    // on that check, so the pass fails and nothing is marked checked.
    fixture.held_mail.fail_next_capture();
    fixture.held_mail.observation_retried();
    checked_while_not_running(&runtime, &held.push_id).await;
    direct_messages_pass_failed(&runtime).await;

    // The backoff checks again, records a position and keeps the mail held on it.
    fixture
        .clock
        .advance(std::time::Duration::from_secs(FIRST_RETRY_SECONDS))
        .unwrap();
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // Its own thread's load, read from that position, releases it.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    journal
        .append(&thread_loaded(&target), fixture.clock.utc_now().timestamp())
        .await
        .unwrap();
    let request = next_request(&runtime, "its own load after the retried capture").await;
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

/// A queued DM to the reader's own session, written straight to the store so no pass
/// runs for it until the next one.
async fn insert_queued_direct_message(fixture: &OwnerFixture) -> PushId {
    let target = target_session(&fixture.reader).unwrap();
    let draft = collaboration_protocol::PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),
        kind: collaboration_protocol::PushKind::DirectMessage,
        origin: collaboration_protocol::PushOrigin::Session(target.clone()),
        origin_router_ref: None,
        target,
        mode: Some(MessageDelivery::Queue),
        guard: None,
        reply_to_push_id: None,
        header_facts: collaboration_protocol::PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("DM checked after the held mail".to_owned()),
        activity: None,
        created_at: fixture.clock.utc_now(),
    };
    let push_id = draft.push_id.clone();
    fixture
        .interactions
        .insert_push_record(draft)
        .await
        .unwrap();
    push_id
}

#[tokio::test]
async fn a_failed_position_capture_still_releases_mail_to_a_running_target() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture.attach_journal().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "released though the capture fails").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;

    // The target runs and its check cannot record a journal position. Nothing will be
    // held on that check, so the release goes ahead in the same pass, with no backoff.
    let clock_at_signal = fixture.clock.monotonic_now();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.held_mail.fail_next_capture();
    fixture.held_mail.provider_completed();
    let request = next_request(&runtime, "release despite the failed capture").await;
    assert_eq!(request.payload.push_id, held.push_id);
    fixture.assert_capture_faults_taken();
    asleep_with_no_failed_pass(&runtime).await;
    assert_eq!(
        fixture.clock.monotonic_now(),
        clock_at_signal,
        "no clock advance"
    );
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn a_later_check_keeps_the_held_mails_earlier_position_so_a_load_between_them_owes_a_check() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let journal = fixture.attach_journal().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "held before the load").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;
    let direct_message = insert_queued_direct_message(&fixture).await;

    // The held mail's check reads "wakeable" and pauses; the thread loads before the
    // same pass checks the DM, which reads "running" and delivers it.
    let (read, read_signal) = tokio::sync::oneshot::channel();
    let (answer_signal, answer) = tokio::sync::oneshot::channel();
    *fixture.presence.2.lock().unwrap() = Some(PresencePause { read, answer });
    fixture.held_mail.observation_retried();
    (read_signal).await.unwrap();
    journal
        .append(&thread_loaded(&target), fixture.clock.utc_now().timestamp())
        .await
        .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    answer_signal.send(()).unwrap();
    checked_while_not_running(&runtime, &held.push_id).await;
    let request = next_request(&runtime, "the DM checked after the load").await;
    assert_eq!(request.payload.push_id, direct_message);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();

    // The load came after the held mail's position: it owes that mail a check.
    let request = next_request(&runtime, "the load between the two checks").await;
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn a_failed_presence_check_marks_nothing_and_the_backoff_checks_again() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "held across a failed check").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;

    // A signal owes a check, and the check fails: the pass fails with nothing marked.
    fixture.presence.fail_next_check();
    fixture.held_mail.observation_retried();
    direct_messages_pass_failed(&runtime).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // The backoff checks again; the target is still not running, so the mail is held.
    fixture
        .clock
        .advance(std::time::Duration::from_secs(FIRST_RETRY_SECONDS))
        .unwrap();
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;
    assert_eq!(
        delivery_state(&native, &held.push_id).await,
        PushDeliveryState::Held
    );

    // The next signal finds it running, and the mail goes.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.held_mail.provider_completed();
    let request = next_request(&runtime, "the load signal after the retried check").await;
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn a_failed_release_unmarks_its_mail_and_the_backoff_releases_it() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let (input, held) = send_held(&app, &target, "released after a failed release").await;
    checked_while_not_running(&runtime, &held.push_id).await;
    asleep_with_nothing_due(&runtime).await;

    // The target runs, but the release transaction fails in storage.
    native_worker_faults::execute(
        fixture.interactions.clone(),
        "CREATE TRIGGER fail_held_release BEFORE UPDATE OF delivery_state ON router_pushes WHEN OLD.delivery_state='held' AND NEW.delivery_state='pending' BEGIN SELECT RAISE(FAIL, 'injected release failure'); END",
        None,
    )
    .await
    .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.held_mail.provider_completed();
    direct_messages_pass_failed(&runtime).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        delivery_state(&native, &held.push_id).await,
        PushDeliveryState::Held
    );
    native_worker_faults::execute(
        fixture.interactions.clone(),
        "DROP TRIGGER fail_held_release",
        None,
    )
    .await
    .unwrap();

    // Nothing stayed marked, so the backoff checks and releases it with no new signal.
    fixture
        .clock
        .advance(std::time::Duration::from_secs(FIRST_RETRY_SECONDS))
        .unwrap();
    let request = next_request(&runtime, "the backoff retried the release").await;
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        reference_for_request(&input, &held.push_id, peer, fixture.clock.as_ref()),
    )
    .await;
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
