//! Recipient custody survives losing its Push receipt, closing and reopening its owner.
use super::{
    composed_fleet_fixture::*, fleet_mail_dispatch::*, fleet_mail_inbox_fixture::RecipientInbox,
};
use fleet_protocol::{DeliveryStatus, PushResult};

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
    let inbox =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    let limits = collaboration_central_core::fleet_outbox_limits()?;
    // The submit's own event pushes to B.
    submit(&fixture, &item).await?;
    let pushed = recipient.push_envelope(item.id()).await?;
    let answer = inbox.accept(pushed.submitter, pushed.item, limits).await?;
    assert!(matches!(answer, PushResult::Accepted { .. }));
    assert_eq!(inbox.count().await?, 1);
    // The actual inbox commit is observed above. Its answer never reaches Fleet.
    let original_call = pushed.call;
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    inbox.close().await?;

    let reopened =
        RecipientInbox::open(&inbox_path, fixture.io.clone(), B.to_owned().try_into()?).await?;
    assert_eq!(reopened.count().await?, 1);
    // Past the unanswered push's first retry time, the next due batch resends
    // it with a new send id and the same call identity.
    fixture.io.advance(31_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let pushed = recipient.push_envelope(item.id()).await?;
    assert_eq!(
        pushed.call, original_call,
        "the resend changed the original Push CallId"
    );
    let answer = reopened
        .accept(pushed.submitter, pushed.item, limits)
        .await?;
    assert!(matches!(
        answer,
        PushResult::Replayed {
            status: DeliveryStatus::AtRecipient,
            ..
        }
    ));
    recipient.answer(&fixture, pushed.frame, answer).await?;
    assert_eq!(reopened.count().await?, 1);
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    reopened.close().await?;
    fixture.close().await
}
