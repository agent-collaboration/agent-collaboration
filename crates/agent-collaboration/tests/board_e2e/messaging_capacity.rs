//! A real recipient quota refuses Push without custody; the same queued items retry.
use super::{fixture::identity, messaging_fixture::MessagingCase, messaging_stopped_owner};
use board_central_test_support::{MailWireEvent, MailWireObservation};
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::FixtureResult;
use fleet_protocol::{DeliveryRefusal, DeliveryTypes, PushResult};
use serde_json::json;
use std::os::unix::fs::OpenOptionsExt;

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;

pub async fn full_inbox(scope: &mut MessagingCase<'_>) -> FixtureResult {
    // Arrange a one-byte quota in the actual recipient Host, before admitting mail.
    let original = identity();
    let following = identity();
    let first_text = "B+ original waits for recipient Inbox capacity";
    let second_text = "B+ following item also waits without a native effect";
    let mut wire = scope
        .recipient
        .enrolled_machine()?
        .observe_mail_wire(None)?;
    let stopped = scope.recipient.stop_for_readback().await?;
    let quota = stopped
        .directory()
        .join(codex_router_host::CollaborationRuntime::RUNTIME_TEST_INBOX_LIMIT_FILE);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&quota)?;
    serde_json::to_writer(file, &json!({"inboxBytes":1}))?;
    drop(stopped);
    scope.recipient.reopen_host().await?;

    // Act through the real sender CLI and inspect correlated recipient wire answers.
    scope.send_cli(&original, first_text).await?;
    require_capacity_refusal(&mut wire, &original).await?;
    scope.send_cli(&following, second_text).await?;
    require_capacity_refusal(&mut wire, &following).await?;
    for item in [&original, &following] {
        scope
            .wait_sender_status(item, &json!({"kind":"sent"}))
            .await?;
    }
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    let stopped = scope.recipient.stop_for_readback().await?;
    let refused = messaging_stopped_owner::delivery_rows(&stopped).await?;
    assert!(
        !refused.iter().any(|row| row.category == "inbox"
            && (row.identity == original || row.identity == following)),
        "capacity refusal admitted recipient custody"
    );

    // Restore room at the already-approved Host lifetime boundary; no SDK rows are edited.
    std::fs::remove_file(quota)?;
    drop(stopped);
    scope.recipient.reopen_host().await?;
    for (item, text) in [(&original, first_text), (&following, second_text)] {
        // A retry may deliver the following item first. Delivered is causal;
        // the native barrier counts both effects without consuming an event
        // that a later ordered expect_marker would need.
        scope
            .wait_sender_status(
                item,
                &json!({
                    "kind":"delivered","acceptance":"peerMessageWritten"
                }),
            )
            .await?;
        assert_eq!(scope.peer()?.count_matching_after_barrier(text).await?, 1);
    }
    assert_eq!(scope.peer()?.count_after_barrier().await?, 2);
    let stopped = scope.recipient.stop_for_readback().await?;
    let delivered = messaging_stopped_owner::delivery_rows(&stopped).await?;
    for item in [&original, &following] {
        assert_eq!(
            delivered
                .iter()
                .filter(|row| row.category == "inbox" && row.identity == *item)
                .count(),
            1
        );
    }
    Ok(())
}

async fn require_capacity_refusal(wire: &mut MailWireObservation, item: &str) -> FixtureResult {
    wire.wait_for(|events| first_push_answer(events, item).is_some())
        .await
        .map_err(|error| {
            format!(
                "capacity Push answer for {item}: {error}; wire={:?}",
                wire.events()
            )
        })?;
    let events = wire.events();
    let answer = first_push_answer(&events, item).ok_or("correlated Push answer disappeared")?;
    assert!(
        matches!(
            answer,
            PushResult::Refused {
                reason: DeliveryRefusal::MailboxFull
            }
        ),
        "recipient Push did not refuse actual capacity: {answer:?}"
    );
    Ok(())
}

fn first_push_answer<'events>(
    events: &'events [MailWireEvent],
    item: &str,
) -> Option<&'events PushResult<Acceptance>> {
    let frame = events.iter().find_map(|event| match event {
        MailWireEvent::PushSent {
            frame,
            item: actual,
        } if actual.as_uuid().to_string() == item => Some(*frame),
        _ => None,
    })?;
    events.iter().find_map(|event| match event {
        MailWireEvent::PushAnswered {
            frame: actual,
            result,
        } if *actual == frame => Some(result),
        _ => None,
    })
}
