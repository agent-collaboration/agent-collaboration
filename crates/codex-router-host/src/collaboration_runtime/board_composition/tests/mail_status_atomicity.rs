//! StatusFetch's actual relay answer is held while the native apply transaction is blocked.
use super::*;
use collaboration_contracts::RouterDeliveryTypes;
use fleet_sync_delivery::{
    DeliveryLimits, Outbox, delivery_rollback, minimum_settlement_allowance,
};

async fn native_status_cursor(root: &Path) -> Result<u64, TestError> {
    let owner = Arc::new(InteractionStore::open(root.join("interaction.sqlite")).await?);
    let store = InteractionAtomicStore::new(owner.clone(), readback_host());
    let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
        64 * 1024 * 1024,
        10_000,
        64 * 1024 * 1024,
        10_000,
        64 * 1024 * 1024,
        minimum_settlement_allowance::<RouterDeliveryTypes>()?,
    )?;
    let cursor = store
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                    .status_cursor()
                    .await
                    .map(|cursor| cursor.value())
                    .map_err(delivery_rollback)
            })
        })
        .await?;
    drop(store);
    Arc::try_unwrap(owner)
        .map_err(|_| io::Error::other("status cursor owning readback retained"))?
        .close()
        .await?;
    Ok(cursor)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn status_prefix_ack_never_precedes_batch_and_cursor_commit_at_sigkill()
-> Result<(), TestError> {
    let (directory, state) = kill_child(CrashKind::StatusBatch).await?;
    let item = state.item.ok_or("status batch crash item missing")?;
    let root = directory.path().join("router");
    let rows = native_rows(&root).await?;
    let cursor = native_status_cursor(&root).await?;
    assert_eq!(header(&rows, item)?["value"]["status"]["kind"], "queued");
    assert_eq!(cursor, 0, "native batch escaped the held writer");
    let origin = origin_rows(directory.path(), state.peer).await?;
    assert!(
        state.status_acknowledged <= cursor,
        "StatusAck reached the real wire before batch and cursor committed at SIGKILL"
    );
    assert!(
        origin
            .iter()
            .any(|row| row.category == "statusEvent" && row.identity == item.as_uuid().to_string()),
        "central retired a status event before native batch commit"
    );

    // Recover through the same public port against the real authoritative stream.
    // This is a separate repair proof, after the at-kill atomicity observation.
    let mut resumed = MailHarness::open_manual(directory.path(), Some(state.peer)).await?;
    resumed.mail.mail.catch_up_status().await?;
    assert_eq!(
        resumed.mail.status_cursor().await?,
        1,
        "status batch did not commit its cursor with the applied statuses"
    );
    assert_eq!(
        header(&resumed.mail.rows().await?, item)?["value"]["status"]["kind"],
        "sent"
    );
    assert!(
        resumed
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::StatusAcked { floor: 1 }))
    );
    resumed.release_welcome()?;
    resumed.close().await
}

pub(super) async fn status_batch_child(base: &Path) -> Result<(), TestError> {
    let mut harness = MailHarness::open_manual(base, None).await?;
    let item = relay_items::ordinary_message(harness.peer());
    let item_id = item.id();
    let envelope = serde_json::from_value(serde_json::to_value(&item)?)?;
    harness.mail.enqueue(item, harness.relay).await?;
    assert!(matches!(
        harness
            .submit_generic(DeliveryEnvelope::Item(envelope))
            .await?,
        fleet_sync_protocol::SubmitResult::Accepted { .. }
    ));
    harness.release_welcome()?;
    assert!(matches!(
        harness.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    assert!(matches!(
        harness.next_manual_event().await?,
        ClientEvent::Request {
            body: ServerRequest::Crate(ServerCall::PublishStatus { .. }),
            ..
        }
    ));
    let (fetched, release) = harness.transport.observer.hold_next_status_fetch();
    let runtime = harness.mail.mail.clone();
    let catching_up = runtime.catch_up_status();
    tokio::pin!(catching_up);
    tokio::select! {
        result = &mut catching_up => panic!("catch-up returned before the actual held relay page: {result:?}"),
        result = fetched => result?,
    }
    // Cursor read is already committed; the real page exists. Only the next
    // native batch transaction is held, without changing any product branch.
    let _held_writer = harness.mail.hold_writer().await?;
    release
        .send(())
        .map_err(|_| io::Error::other("actual status reply gate closed"))?;
    harness
        .transport
        .observer
        .wait_for(
            |events| {
                events.iter().any(|event| {
                    matches!(
                        event,
                        WireEvent::StatusFetched {
                            through: 1,
                            events: 1
                        }
                    )
                })
            },
            "actual authoritative status page was not delivered",
        )
        .await?;
    harness.query(Vec::new()).await?;
    std::future::poll_fn(|cx| {
        assert!(
            catching_up.as_mut().poll(cx).is_pending(),
            "catch-up escaped the held native batch"
        );
        Poll::Ready(())
    })
    .await;
    harness.query(Vec::new()).await?;
    std::future::poll_fn(|cx| {
        assert!(
            catching_up.as_mut().poll(cx).is_pending(),
            "catch-up committed while native writer was held"
        );
        Poll::Ready(())
    })
    .await;
    let status_acknowledged = harness
        .transport
        .observer
        .events()
        .iter()
        .filter_map(|event| match event {
            WireEvent::StatusAck { through } => Some(*through),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let state = CrashState {
        peer: harness.peer(),
        original_frames: Vec::new(),
        published_applied: false,
        item: Some(item_id),
        status_acknowledged,
    };
    std::fs::write(base.join("state.json"), serde_json::to_vec(&state)?)?;
    let mut ready = UnixStream::connect(base.join("ready.sock")).await?;
    ready.write_all(b"R").await?;
    std::future::pending::<Result<(), TestError>>().await
}
