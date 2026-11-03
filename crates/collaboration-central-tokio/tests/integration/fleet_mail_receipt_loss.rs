//! Recipient custody survives losing its Push receipt, closing and reopening its owner.
use super::{
    composed_fleet_fixture::*, fleet_mail_dispatch::*, fleet_mail_inbox_fixture::RecipientInbox,
};
use durable_steps::StoreError;
use fleet_sync_protocol::{DeliveryStatus, PushResult};

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertions distinguish Fleet's retained custody from the independently reopened recipient inbox."
)]
#[tokio::test]
async fn committed_inbox_replays_one_item_after_its_receipt_is_lost_and_the_owner_reopens()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let inbox_path = root.path().join("recipient");
    let inbox = RecipientInbox::open(&inbox_path, fixture.io.clone()).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    let limits = collaboration_central_core::fleet_outbox_limits()?;
    submit(&fixture, &item).await?;
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (_, pushed) = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            let answer = inbox
                .accept(pushed, limits)
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(answer, PushResult::Accepted { .. }));
            assert_eq!(inbox.count().await.map_err(StoreError::backend)?, 1);
            // The actual inbox commit is observed above. Its answer never reaches Fleet.
            let mut deadlines = fixture.io.reply_deadlines.subscribe();
            tokio::time::timeout(CAP, deadlines.wait_for(|count| *count >= 1))
                .await
                .map_err(StoreError::backend)?
                .map_err(StoreError::backend)?;
            fixture.io.advance(2_000);
            Ok::<_, StoreError>(())
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(drive.retried.len(), 1);
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    inbox.close().await?;

    let reopened = RecipientInbox::open(&inbox_path, fixture.io.clone()).await?;
    assert_eq!(reopened.count().await?, 1);
    fixture.io.advance(60_000);
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (frame, pushed) = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            let answer = reopened
                .accept(pushed, limits)
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(
                answer,
                PushResult::Replayed {
                    status: DeliveryStatus::AtRecipient,
                    ..
                }
            ));
            recipient
                .answer(&fixture, frame, answer)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert!(drive.retried.is_empty(), "{drive:?}");
    assert_eq!(reopened.count().await?, 1);
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    reopened.close().await?;
    fixture.close().await
}
