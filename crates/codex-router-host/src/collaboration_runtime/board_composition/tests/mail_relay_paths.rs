//! Actual SDK engine and generic relay; only central Board admission policy stands in.
use super::*;
use fleet_sync_protocol::{DeliveryEnvelope, ItemStatusState};
use transport::WireEvent;

async fn wait_known(
    harness: &MailHarness,
    item: fleet_sync_protocol::ItemId,
) -> Result<
    fleet_sync_protocol::ItemStatusState<collaboration_mail_protocol::CarrierAcceptance>,
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
async fn hints_during_a_real_running_receive_cycle_coalesce_into_one_further_fetch_pass()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    harness.release_welcome()?;
    harness
        .wait_state(
            |state| {
                state["items"]["active"]["cycle"] == 1 && state["controls"]["active"]["cycle"] == 1
            },
            "Welcome did not admit both initial cycles",
        )
        .await?;
    harness.start_mail();
    harness
        .wait_state(
            |state| {
                state["items"]["active"].is_null()
                    && state["controls"]["active"].is_null()
                    && state["items"]["nextCycle"] == 2
                    && state["controls"]["nextCycle"] == 2
            },
            "initial empty relay fetches did not settle",
        )
        .await?;
    let (entered, release) = harness
        .transport
        .observer
        .hold_next_fetch(DeliveryStream::Items);
    harness.transport.publish(1, DeliveryStream::Items).await?;
    tokio::time::timeout(BOUND, entered).await??;
    // The actual generic relay's answer exists, while the real SDK task is
    // still waiting for it. Both hints cross the actual SDK pump and Host loop.
    harness.transport.publish(2, DeliveryStream::Items).await?;
    harness.transport.publish(3, DeliveryStream::Items).await?;
    harness.wait_ack(NoticePosition::new(3)).await?;
    let running = harness
        .mail
        .state()
        .await?
        .ok_or("running receive state missing")?;
    assert_eq!(running["items"]["active"]["cycle"], 2);
    assert_eq!(running["items"]["rerunRequested"], true);
    assert_eq!(running["items"]["nextCycle"], 3);
    release
        .send(())
        .map_err(|_| io::Error::other("real fetch reply gate closed"))?;
    let final_state = harness
        .wait_state(
            |state| state["items"]["active"].is_null() && state["items"]["nextCycle"] == 4,
            "coalesced hint did not complete exactly one further SDK cycle",
        )
        .await?;
    assert_eq!(final_state["items"]["rerunRequested"], false);
    let events = harness.transport.observer.events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                WireEvent::Fetch {
                    stream: DeliveryStream::Items,
                    ..
                }
            ))
            .count(),
        3,
        "initial, running and one coalesced pass must be the only item fetches"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, WireEvent::Received(DeliveryStream::Items)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                WireEvent::Fetch {
                    stream: DeliveryStream::Controls,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(harness.connections(), 1);
    harness.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn submit_fetch_received_report_and_committed_status_use_one_actual_session()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    let item = relay_items::ordinary_message(harness.peer());
    let id = item.id();
    let digest = item.digest().clone();
    // Only setup admits this outbox task. The actual runtime must register and
    // execute the SDK SubmitItem kind, then retain its relay's canonical status.
    harness.mail.enqueue(item, harness.relay).await?;
    harness.release_welcome()?;
    harness
        .wait_state(
            |state| {
                state["items"]["active"]["cycle"] == 1 && state["controls"]["active"]["cycle"] == 1
            },
            "Welcome did not admit real receive work",
        )
        .await?;
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
    harness.transport.publish(1, DeliveryStream::Items).await?;
    let observed = harness.transport.observer.wait_for(|events| {
        let held_frame = events.iter().find_map(|event| match event {
            WireEvent::Publication { frame, item, status: fleet_sync_protocol::DeliveryStatus::Held } if *item == id => Some(frame),
            _ => None,
        });
        events.iter().any(|event| matches!(event, WireEvent::Fetched { items, .. } if items.contains(&id)))
            && events.iter().any(|event| matches!(event, WireEvent::Received(DeliveryStream::Items)))
            && events.iter().any(|event| matches!(event, WireEvent::Report(item) if *item == id))
            && held_frame.is_some_and(|held| events.iter().any(|event|
                matches!(event, WireEvent::Published { frame, applied: true } if frame == held)))
    }, "real relay round trip did not finish fetch/receipt/report/canonical publication").await;
    if observed.is_err() {
        println!(
            "actual relay wire events: {:?}",
            harness.transport.observer.events()
        );
        println!(
            "actual native SDK rows: {}",
            serde_json::to_string(&harness.mail.rows().await?)?
        );
        println!(
            "actual native SDK receive state: {:?}",
            harness.mail.state().await?
        );
    }
    let events = observed?;
    assert!(
        events.iter().any(|event| matches!(
            event,
            WireEvent::Fetch { stream: DeliveryStream::Items, inbox_room_bytes: Some(room), .. }
                if *room > 0
        )),
        "the real Router session lost the SDK receiving-room budget"
    );
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
    assert_eq!(status, fleet_sync_protocol::DeliveryStatus::Held);
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
async fn largest_accepted_items_are_returned_by_the_relay_under_the_actual_runtime_budget()
-> Result<(), TestError> {
    // Each item gets a fresh physical mailbox/owner. A malformed native projection
    // cannot hide the other item's independent Fetch observation.
    for notification in [false, true] {
        let mut harness = MailHarness::open(false).await?;
        let item = if notification {
            relay_items::largest_notification(harness.peer())
        } else {
            relay_items::largest_message(harness.peer())
        };
        let id = item.id();
        let envelope: DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes> =
            serde_json::from_value(serde_json::to_value(DeliveryEnvelope::Item(item))?)?;
        assert!(matches!(
            harness.submit_generic(envelope).await?,
            fleet_sync_protocol::SubmitResult::Accepted { .. }
        ));
        harness.release_welcome()?;
        harness
            .wait_state(
                |state| state["items"]["active"]["cycle"] == 1,
                "Welcome did not admit the maximal item fetch",
            )
            .await?;
        harness.start_mail();
        let observed = harness.transport.observer.wait_for(|events| events.iter().any(|event|
            matches!(event, WireEvent::Fetched { stream: DeliveryStream::Items, items, .. } if items.contains(&id))),
            "maximal accepted item was stranded by the runtime fetch byte budget").await;
        if observed.is_err() {
            println!(
                "maximal item actual wire events: {:?}",
                harness.transport.observer.events()
            );
            println!(
                "maximal item retained central custody: {:?}",
                harness.query(vec![id]).await?
            );
        }
        let events = observed?;
        let budgets = events
            .iter()
            .filter_map(|event| match event {
                WireEvent::Fetch {
                    stream: DeliveryStream::Items,
                    max_bytes,
                    ..
                } => Some(*max_bytes),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!budgets.is_empty());
        assert!(budgets.iter().all(|budget| *budget == fleet_sync_protocol::unit_budget::TARGET_UNIT_BYTES as u64));
        let sizes = events
            .iter()
            .filter_map(|event| match event {
                WireEvent::Fetched {
                    items, entry_bytes, ..
                } if items.contains(&id) => Some(entry_bytes),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        println!(
            "real relay maximal notification={notification}; returned entry bytes={sizes:?}; runtime budgets={budgets:?}"
        );
        assert!(
            !sizes.is_empty(),
            "maximal Fetch observation had no measured entries"
        );
        assert!(
            sizes
                .iter()
                .all(|size| *size <= fleet_sync_protocol::unit_budget::TARGET_UNIT_BYTES)
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
    let wrong_relay = fleet_sync_protocol::PeerId::new(durable_steps::Uuid::now_v7())?;
    assert_ne!(wrong_relay, harness.relay);
    let (_, wrong_operation) = harness.mail.enqueue(wrong_item, wrong_relay).await?;
    let correct = relay_items::ordinary_message(harness.peer());
    let correct_id = correct.id();
    harness.mail.enqueue(correct, harness.relay).await?;
    harness.release_welcome()?;
    harness
        .wait_state(
            |state| state["items"]["active"]["cycle"] == 1,
            "Welcome did not admit receive work",
        )
        .await?;
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
