//! Authoritative status pages cross the same real session, including a lost prefix ACK.
use super::*;
use fleet_sync_protocol::{DeliveryEnvelope, SubmitResult};
use transport::WireEvent;

#[tokio::test]
async fn welcome_catch_up_commits_pages_and_resumes_after_a_lost_status_ack()
-> Result<(), TestError> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let mut harness = MailHarness::open(false).await?;
    let item = relay_items::ordinary_message(harness.peer());
    let item_id = item.id();
    let envelope = serde_json::from_value(serde_json::to_value(&item)?)?;
    harness.mail.enqueue(item, harness.relay).await?;
    assert!(matches!(
        harness
            .submit_generic(DeliveryEnvelope::Item(envelope))
            .await?,
        SubmitResult::Accepted { .. }
    ));
    // These are real central admissions and real status-stream publications.
    // Unknown local items are legal catch-up entries, not fabricated native headers.
    for _ in 0..100 {
        let item = relay_items::ordinary_message(harness.peer());
        let envelope = serde_json::from_value(serde_json::to_value(item)?)?;
        assert!(matches!(
            harness
                .submit_generic(DeliveryEnvelope::Item(envelope))
                .await?,
            SubmitResult::Accepted { .. }
        ));
    }
    tokio::time::timeout(BOUND, async {
        loop {
            let batch = harness.status_page(0, 101).await.map_err(|error| {
                io::Error::other(format!(
                    "authoritative setup StatusFetch failed: {error}; actual wire: {:?}",
                    harness.transport.observer.events()
                ))
            })?;
            if batch.through.value() == 101 {
                assert_eq!(batch.events.len(), 101);
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| io::Error::other("central status stream did not acquire all 101 events"))??;
    let first_runtime_event = harness.transport.observer.events().len();
    assert_eq!(harness.mail.status_cursor().await?, 0);
    harness.transport.observer.lose_next_status_ack();
    harness.release_welcome()?;
    let events = harness
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .filter(|event| matches!(event, WireEvent::StatusAcked { floor: 101 }))
                    .count()
                    == 2
            },
            "Welcome catch-up did not retry from its committed cursor and ACK its empty page",
        )
        .await?;
    let events = &events[first_runtime_event..];
    let fetches: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            WireEvent::StatusFetch { after, limit } => Some((*after, *limit)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fetches,
        vec![(0, 100), (100, 100), (101, 100)],
        "the next actual Welcome must fetch after the durable first page"
    );
    let pages: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            WireEvent::StatusFetched { through, events } => Some((*through, *events)),
            _ => None,
        })
        .collect();
    assert_eq!(pages, vec![(100, 100), (101, 1), (101, 0)]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WireEvent::LostStatusAck { through: 100 }))
    );
    let sent_acks: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            WireEvent::StatusAck { through } => Some(*through),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent_acks,
        vec![101, 101],
        "only the first prefix ACK was lost; empty-page ACK must retry the committed prefix"
    );
    assert_eq!(harness.mail.status_cursor().await?, 101);
    let rows = harness.mail.rows().await?;
    let row = rows
        .iter()
        .find(|row| row.category == "outbox" && row.identity == item_id.as_uuid().to_string())
        .ok_or("known sender header missing after catch-up")?;
    let header: serde_json::Value =
        serde_json::from_str(row.content.as_ref().ok_or("header missing")?)?;
    assert_eq!(header["value"]["status"]["kind"], "sent");
    assert_eq!(row.payload_bytes.as_deref(), Some("0"));
    assert_eq!(
        harness.connections(),
        2,
        "the same pump must reconnect after the simulated transport loss"
    );
    // The separate Welcome receive handlers also commit while status paging runs.
    let state = harness
        .mail
        .state()
        .await?
        .ok_or("independent Welcome receives missing")?;
    assert_eq!(state["items"]["active"]["cycle"], 1);
    assert_eq!(state["controls"]["active"]["cycle"], 1);
    harness.close().await
}
