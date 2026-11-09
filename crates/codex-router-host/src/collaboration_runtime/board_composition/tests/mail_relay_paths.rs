//! Actual SDK engine and generic relay; only central Board admission policy stands in.
use super::*;
use fleet_protocol::{DeliveryEnvelope, ItemStatusState};
use transport::WireEvent;

async fn wait_known(
    harness: &MailHarness,
    item: fleet_protocol::ItemId,
) -> Result<
    fleet_protocol::ItemStatusState<collaboration_mail_protocol::CarrierAcceptance>,
    TestError,
> {
    tokio::time::timeout(BOUND, async {
        loop {
            let answer = harness.query(vec![item]).await?;
            if let Some(status) = answer
                .statuses
                .into_iter()
                .find(|status| status.item == item)
                && matches!(status.state, ItemStatusState::Known { .. })
            {
                return Ok(status.state);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| io::Error::other("real relay did not acquire submitted custody"))?
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn submit_push_report_and_committed_status_use_one_actual_session() -> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    let item = relay_items::ordinary_message(harness.peer());
    let id = item.id();
    let digest = item.digest().clone();
    // Only setup admits this outbox task. The actual runtime must register and
    // execute the SDK SubmitItem kind, then retain its relay's canonical status.
    harness.mail.enqueue(item, harness.relay).await?;
    pushed_mail::welcome(&mut harness).await?;
    harness.start_mail();
    harness
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, WireEvent::MailSubmit(sent) if *sent == id))
            },
            "runtime never sent Board MailSubmit",
        )
        .await?;
    wait_known(&harness, id).await?;
    // FleetService sends the real Push; the correlated receipt below must be Accepted.
    let observed = harness
        .transport
        .observer
        .wait_for(
            |events| {
                let held_frame = events.iter().find_map(|event| match event {
                    WireEvent::Publication {
                        frame,
                        item,
                        status: fleet_protocol::DeliveryStatus::Held,
                    } if *item == id => Some(frame),
                    _ => None,
                });
                events.iter().any(|event| {
                    matches!(
                        event,
                        WireEvent::Pushed {
                            result: fleet_protocol::PushResult::Accepted { .. },
                            ..
                        }
                    )
                }) && events
                    .iter()
                    .any(|event| matches!(event, WireEvent::Report(item) if *item == id))
                    && held_frame.is_some_and(|held| {
                        events.iter().any(|event|
                matches!(event, WireEvent::Published { frame, applied: true } if frame == held))
                    })
            },
            "real session did not finish Push/report/canonical publication",
        )
        .await;
    if observed.is_err() {
        println!(
            "actual relay wire events: {:?}",
            harness.transport.observer.events()
        );
        println!(
            "actual native SDK rows: {}",
            serde_json::to_string(&harness.mail.rows().await?)?
        );
    }
    let events = observed?;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WireEvent::GenericSubmit(item) if *item == id))
    );
    let canonical = wait_known(&harness, id).await?;
    let ItemStatusState::Known {
        status, version, ..
    } = canonical
    else {
        return Err("canonical status disappeared".into());
    };
    // Auto intentionally holds when no route currently claims the target.
    // Keep that policy and require the exact canonical report/publication.
    assert_eq!(status, fleet_protocol::DeliveryStatus::Held);
    let rows = harness.mail.rows().await?;
    let row = rows
        .iter()
        .find(|row| row.category == "outbox" && row.identity == id.as_uuid().to_string())
        .ok_or("native outbox missing")?;
    let header: serde_json::Value =
        serde_json::from_str(row.content.as_deref().ok_or("outbox header missing")?)?;
    assert_eq!(header["kind"], "item");
    assert_eq!(header["value"]["status"], serde_json::to_value(status)?);
    assert_eq!(header["value"]["version"], serde_json::to_value(version)?);
    assert_eq!(header["value"]["digest"], digest.as_str());
    assert_eq!(row.payload_bytes.as_deref(), Some("0"));
    assert_eq!(
        harness.mail.status_cursor().await?,
        0,
        "live publication skipped catch-up history"
    );
    assert_eq!(
        harness.connections(),
        1,
        "mail opened an additional session"
    );
    harness.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn largest_pushed_items_commit_through_the_actual_machine_session() -> Result<(), TestError> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    for notification in [false, true] {
        // Arrange independent real physical inboxes for each maximum legal envelope.
        let mut harness = MailHarness::open(false).await?;
        let item = if notification {
            relay_items::largest_notification(harness.peer())
        } else {
            relay_items::largest_message(harness.peer())
        };
        let id = item.id();
        let envelope = serde_json::from_value(serde_json::to_value(DeliveryEnvelope::Item(item))?)?;
        pushed_mail::welcome(&mut harness).await?;
        // Act through a correlated typed frame, real pump, Host and Inbox.
        assert!(matches!(
            pushed_mail::push(&harness, envelope).await?,
            fleet_protocol::PushResult::Accepted { .. }
        ));
        // Assert maximum inputs commit once without admitting a recipient Fetch.
        assert_eq!(
            harness
                .mail
                .rows()
                .await?
                .iter()
                .filter(|row| row.category == "inbox" && row.identity == id.as_uuid().to_string())
                .count(),
            1
        );

        assert_eq!(harness.connections(), 1);
        harness.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn wrong_relay_is_never_sent_while_a_correct_submission_on_the_same_session_succeeds()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    let wrong_item = relay_items::ordinary_message(harness.peer());
    let wrong_id = wrong_item.id();
    let wrong_relay = fleet_protocol::PeerId::new(durable_steps::Uuid::now_v7())?;
    assert_ne!(wrong_relay, harness.relay);
    let (_, wrong_operation) = harness.mail.enqueue(wrong_item, wrong_relay).await?;
    let correct = relay_items::ordinary_message(harness.peer());
    let correct_id = correct.id();
    harness.mail.enqueue(correct, harness.relay).await?;
    pushed_mail::welcome(&mut harness).await?;
    harness.start_mail();
    wait_known(&harness, correct_id).await?;
    tokio::time::timeout(BOUND, async {
        loop {
            let events = harness.transport.observer.events();
            if events.iter().any(|event| matches!(event, WireEvent::MailSubmit(item) if *item == wrong_id)) {
                return Err::<(), TestError>(io::Error::other("wrong relay reached the actual session wire").into());
            }
            let (operation, steps) = harness.mail.operation(wrong_operation).await?;
            let attempted = operation.is_some_and(|operation| matches!(operation.execution,
                durable_steps::storage::OperationExecution::Running { attempt, drive: None, .. } if attempt >= 1));
            let not_sent = steps.iter().any(|step| step.name.as_str() == "submit" && matches!(&step.state,
                durable_steps::storage::StepState::CallPending { attempt: durable_steps::storage::CallAttempt {
                    reach: durable_steps::storage::CallReach::NeverSent, .. }, .. }));
            if attempted && not_sent { return Ok(()); }
            tokio::task::yield_now().await;
        }
    }).await.map_err(|_| io::Error::other("wrong relay did not record a completed NeverSent attempt"))??;
    let answer = harness.query(vec![wrong_id]).await?;
    assert!(
        matches!(answer.statuses.as_slice(), [status] if status.item == wrong_id && matches!(status.state, ItemStatusState::Unknown))
    );
    assert!(
        harness
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::MailSubmit(item) if *item == correct_id))
    );
    assert_eq!(harness.connections(), 1);
    harness.close().await
}
