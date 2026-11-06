//! Enrollment forgets the former stream without throwing away sender custody.
use super::*;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryStatus, RecipientOutcome, SubmitResult};
use sqlx::Connection;
use transport::WireEvent;

async fn seed_old_cursor(root: &Path) -> Result<(), TestError> {
    let mut connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(root.join("interaction.sqlite")),
    )
    .await?;
    sqlx::query("INSERT INTO _fleet_sync_outbox_status(singleton,cursor,created_at,updated_at) VALUES(1,100,1,1) ON CONFLICT(singleton) DO UPDATE SET cursor=100")
        .execute(&mut connection).await?;
    connection.close().await?;
    Ok(())
}

fn retained_custody(
    rows: Vec<fleet_sync_delivery::test_support::DeliveryObservation>,
) -> Result<Vec<serde_json::Value>, TestError> {
    rows.into_iter()
        .filter(|row| matches!(row.category.as_str(), "outbox" | "inbox" | "report"))
        .map(|row| serde_json::to_value(row).map_err(Into::into))
        .collect()
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture failures propagate."
)]
async fn reenrollment_resets_old_status_cursor_and_reconnect_catches_new_mail_without_live_hints()
-> Result<(), TestError> {
    // Arrange an old learned cursor beside real pending custody. Fleet is already
    // fresh and stays at this same URL/peer/relay throughout reset and reconnect.
    let directory = tempfile::tempdir()?;
    let first = MailHarness::open_manual(directory.path(), None).await?;
    let peer = first.peer();
    let retained = relay_items::ordinary_message(peer);
    first.mail.enqueue(retained, first.relay).await?;
    seed_old_cursor(&first.root).await?;
    assert_eq!(first.mail.status_cursor().await?, 100);
    assert_eq!(first.status_page(0, 100).await?.through.value(), 0);
    let custody_before = retained_custody(first.mail.rows().await?)?;
    assert!(!custody_before.is_empty());
    let central = first.close_keeping_central().await?;
    let storage = directory.path().join("router");
    let journal = directory.path().join("enrollment");

    // Act through the real resumable reset before a new machine session loads.
    drop(crate::enrollment_reset::record_pending_enrollment_reset(
        &journal,
    )?);
    crate::enrollment_reset::resume_enrollment_reset(&journal, &storage).await?;
    assert!(!crate::enrollment_reset::reset_pending_in_directory(
        &journal
    )?);
    let second = MailHarness::open_configured(
        directory.path(),
        None,
        false,
        true,
        Some(peer),
        Some(central),
    )
    .await?;
    assert_eq!(retained_custody(second.mail.rows().await?)?, custody_before);
    let message = relay_items::ordinary_message(peer);
    let item = message.id();
    let envelope = serde_json::from_value(serde_json::to_value(&message)?)?;
    second.mail.enqueue(message, second.relay).await?;
    assert!(matches!(
        second
            .submit_generic(DeliveryEnvelope::Item(envelope))
            .await?,
        SubmitResult::Accepted { .. }
    ));
    second
        .report_outcome(
            item,
            RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            },
        )
        .await?;
    let through = tokio::time::timeout(BOUND, async {
        loop {
            let batch = second.status_page(0, 100).await?;
            if batch.events.iter().any(|event| {
                event.item == item && matches!(event.status, DeliveryStatus::Delivered { .. })
            }) {
                return Ok::<_, TestError>(batch.through.value());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert!((1..100).contains(&through));
    // Neither lifetime runs its event owner. PublishStatus hints stay unconsumed.
    let central = second.close_keeping_central().await?;
    let mut reconnected = MailHarness::open_configured(
        directory.path(),
        None,
        false,
        true,
        Some(peer),
        Some(central),
    )
    .await?;
    reconnected.release_welcome()?;
    assert!(matches!(
        reconnected.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    reconnected
        .mail
        .mail
        .catch_up_status()
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "fresh-stream catch-up failed after enrollment reset: {error}; wire: {:?}",
                reconnected.transport.observer.events()
            ))
        })?;

    // Assert the normal product catch-up applied and ACKed the lower fresh prefix.
    assert_eq!(reconnected.mail.status_cursor().await?, through);
    assert!(matches!(
        reconnected.mail.outbox_status(item).await?.state,
        fleet_sync_delivery::OutboxStatusState::Known {
            status: DeliveryStatus::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
            },
            ..
        }
    ));
    let events = reconnected.transport.observer.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WireEvent::StatusFetch { after: 0, .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WireEvent::StatusAcked { floor } if *floor == through))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, WireEvent::Published { applied: true, .. })),
        "a live hint hid a broken catch-up"
    );
    reconnected.close().await
}
