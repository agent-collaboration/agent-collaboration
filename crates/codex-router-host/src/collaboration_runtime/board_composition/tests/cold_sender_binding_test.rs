//! Cold sender authority is resolved through the live session before Push is answered.
use super::*;
use board_sync_protocol::{BoardDeliveryTypes, InstallationPeerLookup};
use collaboration_mail_protocol::MailSender;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, PeerId, PushResult};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn cold_recipient_resolves_sender_and_retries_push_without_a_background_worker()
-> Result<(), TestError> {
    // Arrange a welcomed recipient with no sender directory entry and no mail runner.
    let mut harness = MailHarness::open(false).await?;
    pushed_mail::welcome(&mut harness).await?;
    let service: message_board::ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let sender = PeerId::new(uuid::Uuid::now_v7())?;
    harness
        .central
        .directory
        .answer(
            service.clone(),
            InstallationPeerLookup::Enrolled { peer: sender },
        )
        .await;
    let (entered, release) = harness.central.directory.hold_next().await;
    assert!(
        harness
            .mail
            .owner
            .installation_peer(&service)
            .await?
            .is_none()
    );
    assert!(!harness.mail_started);
    let template = relay_items::ordinary_message(harness.peer());
    let item = DeliveryItem::<BoardDeliveryTypes>::new(
        template.id(),
        MailSender::HumanUser {
            installation: service.clone(),
        },
        template.recipient().clone(),
        template.valid_until(),
        template.body().clone(),
    )?;
    let (frame, bytes) = pushed_mail::push_frame(sender, DeliveryEnvelope::Item(item))?;
    // Act through the actual pump and Host Push adapter. Hold only the authenticated lookup.
    harness.transport.replay(vec![bytes.into()]).await?;
    tokio::select! {
        entered = tokio::time::timeout(BOUND, entered) => entered??,
        answered = harness.transport.observer.wait_for(|events| events.iter().any(|event|
            matches!(event, transport::WireEvent::Pushed { frame: actual, .. } if *actual == frame)),
            "Push neither requested sender authority nor answered") => {
            answered?;
            panic!("cold Push answered before authenticated sender lookup");
        }
    };
    // The failed custody scope must have released the physical owner before network I/O.
    assert!(
        tokio::time::timeout(BOUND, harness.mail.owner.installation_peer(&service))
            .await??
            .is_none()
    );
    assert!(!harness.transport.observer.events().iter().any(|event|
        matches!(event, transport::WireEvent::Pushed { frame: actual, .. } if *actual == frame)));
    release
        .send(())
        .map_err(|_| io::Error::other("sender lookup caller closed"))?;
    let events = harness
        .transport
        .observer
        .wait_for(
            |events| {
                events.iter().any(|event|
        matches!(event, transport::WireEvent::Pushed { frame: actual, .. } if *actual == frame))
            },
            "cold Push was not retried after sender lookup",
        )
        .await?;
    // Assert the same request acquired custody immediately, without a runner or timer advance.
    assert!(events.iter().any(|event| matches!(event,
        transport::WireEvent::Pushed { frame: actual, result: PushResult::Accepted { .. } } if *actual == frame)));
    assert_eq!(harness.central.directory.read_count(&service).await, 1);
    assert_eq!(
        harness
            .mail
            .owner
            .installation_peer(&service)
            .await?
            .ok_or("sender binding missing")?
            .as_str(),
        sender.as_uuid().to_string()
    );
    assert!(!harness.mail_started);
    harness.close().await
}
