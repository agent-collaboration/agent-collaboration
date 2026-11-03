//! Fleet's saved custody answers replay and conflict across the actual store owner reopening.
use super::{
    composed_fleet_fixture::*, fleet_mail_dispatch::*, fleet_mail_inbox_fixture::RecipientInbox,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, PushResult};
use serde_json::json;
use sqlx_core::{query::query, row::Row};

async fn admitted_rows(fixture: &Fixture) -> TestResult<(i64, i64)> {
    Ok(fixture.store.transaction(|scope| Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM _fleet_sync_outbox_items) AS items,(SELECT count(*) FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.push-item') AS pushes")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("items").map_err(StoreError::backend)?,row.try_get("pushes").map_err(StoreError::backend)?))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare replayed admission, conflicting immutable content and independently committed recipient custody across reopen."
)]
#[tokio::test]
async fn replay_and_conflict_preserve_one_fleet_custody_row_and_a_restart_still_pushes_it()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fleet_path = root.path().join("fleet");
    let fixture = Fixture::open(&fleet_path, NOW).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let replay = submit_answer(&fixture, &item).await?;
    assert_eq!(
        replay.pointer("/value/value/kind"),
        Some(&json!("replayed")),
        "{replay}"
    );
    assert_eq!(
        replay.pointer("/value/value/value/status/kind"),
        Some(&json!("sent")),
        "{replay}"
    );
    let DeliveryEnvelope::Item(original) = &item else {
        return Err("expected immutable item".into());
    };
    let conflict = DeliveryEnvelope::Item(DeliveryItem::new(
        original.id(),
        original.sender().clone(),
        original.recipient().clone(),
        None,
        serde_json::from_value(
            json!({"kind":"message","text":"different immutable content","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?);
    let answer = submit_answer(&fixture, &conflict).await?;
    assert_eq!(
        answer.pointer("/value/value/kind"),
        Some(&json!("refused")),
        "{answer}"
    );
    assert_eq!(
        answer.pointer("/value/value/value/reason/kind"),
        Some(&json!("itemConflict")),
        "{answer}"
    );
    assert_eq!(admitted_rows(&fixture).await?, (1, 1));
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    fixture.close().await?;

    let reopened = Fixture::open(&fleet_path, NOW + 10_000).await?;
    let replay = submit_answer(&reopened, &item).await?;
    assert_eq!(
        replay.pointer("/value/value/kind"),
        Some(&json!("replayed")),
        "{replay}"
    );
    assert_eq!(admitted_rows(&reopened).await?, (1, 1));
    let mut recipient = Socket::open(&reopened, B, SERVICE_B).await?;
    let inbox = RecipientInbox::open(&root.path().join("recipient"), reopened.io.clone()).await?;
    reopened.io.advance(2_000);
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(reopened.runtime.on_alarm(), async {
            let (frame, pushed) = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            assert_eq!(
                pushed.digest(),
                item.digest(),
                "the conflicting attempt must not replace custody"
            );
            let answer = inbox
                .accept(
                    pushed,
                    collaboration_central_core::central_mailbox_limits()
                        .map_err(StoreError::backend)?,
                )
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(answer, PushResult::Accepted { .. }));
            recipient
                .answer(&reopened, frame, answer)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert!(drive.retried.is_empty(), "{drive:?}");
    assert_eq!(inbox.count().await?, 1);
    assert_eq!(
        custody(&reopened, item.id()).await?,
        ("atRecipient".into(), false)
    );
    let replay = submit_answer(&reopened, &item).await?;
    assert_eq!(
        replay.pointer("/value/value/value/status/kind"),
        Some(&json!("atRecipient")),
        "{replay}"
    );
    // PushItem discards its completed journal row. Replaying the handoff must admit no new Push.
    assert_eq!(admitted_rows(&reopened).await?, (1, 0));
    inbox.close().await?;
    reopened.close().await
}
