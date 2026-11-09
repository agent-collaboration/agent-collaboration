//! Push-wait TTL through real Fleet tasks, recipient custody and owning reopens.
use super::{
    composed_fleet_fixture::*, fleet_mail_dispatch::*, fleet_mail_inbox_fixture::RecipientInbox,
    fleet_mail_query::query,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::ObjectHost;
use durable_steps::StoreError;
use fleet_protocol::{
    DeliveryAnswer, DeliveryEnvelope, DeliveryItem, DeliveryStatus, ItemStatusState, PushResult,
    StatusVersion,
};
use runtime_primitives::SyncTime;

fn item(ttl: bool) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    let DeliveryEnvelope::Item(original) = envelope(B, SERVICE_B)? else {
        return Err("mail fixture returned a cancel".into());
    };
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        original.id(),
        original.sender().clone(),
        original.recipient().clone(),
        ttl.then_some(SyncTime::from_unix_millis(NOW + 1_000)),
        original.body().clone(),
    )?))
}

async fn deliver(
    fixture: &Fixture,
    socket: &mut Socket,
    inbox: &RecipientInbox,
    expected: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult {
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let pushed = socket
                .push_envelope(expected.id())
                .await
                .map_err(StoreError::backend)?;
            assert_eq!(
                serde_json::to_vec(&pushed.item).map_err(StoreError::backend)?,
                serde_json::to_vec(expected).map_err(StoreError::backend)?
            );
            let receipt = inbox
                .accept(
                    pushed.submitter,
                    pushed.item,
                    collaboration_central_core::fleet_outbox_limits()
                        .map_err(StoreError::backend)?,
                )
                .await
                .map_err(StoreError::backend)?;
            assert!(matches!(receipt, PushResult::Accepted { .. }));
            socket
                .answer(fixture, pushed.frame, receipt)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    Ok(())
}

async fn require_expired(
    fixture: &Fixture,
    sender: &mut Socket,
    expired: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult {
    let DeliveryAnswer::Query(answer) = query(fixture, sender, vec![expired.id()]).await? else {
        return Err("expired item lost its authenticated Query result".into());
    };
    assert!(
        matches!(answer.statuses.as_slice(),[state] if state.item == expired.id()
        && matches!(&state.state,ItemStatusState::Known {digest,status:DeliveryStatus::Expired,version,cancel:None}
            if digest==expired.digest() && *version == StatusVersion::new(2)))
    );
    assert_eq!(
        custody(fixture, expired.id()).await?,
        ("expired".into(), false)
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A definitely-unsent item's saved deadline must still expire after the Fleet closes before due, before any sender read or B connection, without adding recipient custody or work."
)]
async fn push_wait_deadline_survives_fleet_reopen_before_due_without_any_recipient_work()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fleet_path = root.path().join("fleet");
    let inbox_path = root.path().join("recipient");
    let fixture = Fixture::open(&fleet_path, NOW).await?;
    let inbox =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    let expired = item(true)?;
    submit(&fixture, &expired).await?;
    let report = fixture.runtime.on_alarm().await?;
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(
        custody(&fixture, expired.id()).await?,
        ("sent".into(), true)
    );
    assert_eq!(inbox.count().await?, 0);
    assert!(inbox.work_ids().await?.is_empty());
    inbox.close().await?;
    fixture.close().await?;

    let fixture = Fixture::open(&fleet_path, NOW + 1_001).await?;
    let inbox =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    // Drive the real recovered runtime before a query or either socket opens.
    let report = fixture.runtime.on_alarm().await?;
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(
        custody(&fixture, expired.id()).await?,
        ("expired".into(), false)
    );
    assert_eq!(inbox.count().await?, 0);
    assert!(inbox.work_ids().await?.is_empty());
    assert!(!inbox.contains(expired.id()).await?);
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    require_expired(&fixture, &mut sender, &expired).await?;
    inbox.close().await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual SDK recipient commits and exact work identities distinguish offline expiry from accepted or no-TTL custody across owning reopens."
)]
#[tokio::test]
async fn offline_expiry_creates_no_recipient_work_and_survives_reopen_while_receipted_and_no_ttl_remain()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fleet_path = root.path().join("fleet");
    let inbox_path = root.path().join("recipient");
    let fixture = Fixture::open(&fleet_path, NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let inbox =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    let receipted = item(true)?;
    submit(&fixture, &receipted).await?;
    deliver(&fixture, &mut recipient, &inbox, &receipted).await?;
    assert_eq!(
        custody(&fixture, receipted.id()).await?,
        ("atRecipient".into(), false)
    );
    let work_before = inbox.work_ids().await?;
    assert!(
        !work_before.is_empty(),
        "positive recipient admission created no work"
    );

    // Commit closed registration and physically quarantine B before admitting
    // the offline items. Socket closure is a causal input, never a sleep.
    fixture.runtime.on_socket_closed(recipient.binding).await?;
    fixture
        .host
        .quarantine_connections(vec![recipient.binding.connection])
        .await;
    drop(recipient);
    let expired = item(true)?;
    let no_ttl = item(false)?;
    submit(&fixture, &expired).await?;
    submit(&fixture, &no_ttl).await?;
    let drive = tokio::time::timeout(CAP, fixture.runtime.on_alarm()).await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(
        custody(&fixture, expired.id()).await?,
        ("sent".into(), true)
    );
    assert_eq!(custody(&fixture, no_ttl.id()).await?, ("sent".into(), true));
    fixture.io.advance(1_001);
    let drive = tokio::time::timeout(CAP, fixture.runtime.on_alarm()).await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    require_expired(&fixture, &mut sender, &expired).await?;
    assert_eq!(
        custody(&fixture, receipted.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(custody(&fixture, no_ttl.id()).await?, ("sent".into(), true));
    assert_eq!(inbox.count().await?, 1);
    assert!(!inbox.contains(expired.id()).await?);
    assert!(!inbox.contains(no_ttl.id()).await?);
    assert_eq!(
        inbox.work_ids().await?,
        work_before,
        "expiry added recipient work"
    );
    drop(sender);
    inbox.close().await?;
    fixture.close().await?;

    let fixture = Fixture::open(&fleet_path, NOW + 1_001).await?;
    let inbox =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    require_expired(&fixture, &mut sender, &expired).await?;
    assert_eq!(inbox.count().await?, 1);
    assert_eq!(inbox.work_ids().await?, work_before);
    assert!(!inbox.contains(expired.id()).await?);
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    fixture.io.advance(60_000);
    deliver(&fixture, &mut recipient, &inbox, &no_ttl).await?;
    assert_eq!(inbox.count().await?, 2);
    assert!(
        !inbox.contains(expired.id()).await?,
        "expired Push entered recipient custody after reopen"
    );
    assert!(inbox.contains(receipted.id()).await?);
    assert!(inbox.contains(no_ttl.id()).await?);
    require_expired(&fixture, &mut sender, &expired).await?;
    assert_eq!(
        custody(&fixture, receipted.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(
        custody(&fixture, no_ttl.id()).await?,
        ("atRecipient".into(), false)
    );
    inbox.close().await?;
    fixture.close().await
}
