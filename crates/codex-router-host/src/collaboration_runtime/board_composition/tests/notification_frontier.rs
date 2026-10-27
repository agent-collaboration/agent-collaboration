//! A real frontier commit, writer ACK and Host lifecycle prompt release local retirement.
use super::notification_owner::ProjectNotificationFixture;
use super::*;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::NotificationSeed;
use collaboration_service::ProjectReplicationPosition;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, ItemId, RecipientOutcome};
use message_board::{HandoffSeq, MailItemRef};
use message_board_storage::MailHandoffItem;

#[tokio::test]
async fn local_custody_waits_for_the_real_frontier_ack_and_host_prompt_before_retirement()
-> Result<(), TestError> {
    // Arrange the actual default Host writer, receiver, mail owner and event loop.
    let directory = tempfile::tempdir()?;
    let mut fixture = Box::pin(ProjectNotificationFixture::open_local(directory.path())).await?;
    fixture.start().await?;
    let peer = fixture.harness.peer();
    let item = fixture.seed_item;
    tokio::time::timeout(BOUND, async {
        loop {
            let marks = fixture
                .harness
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?;
            let outcome = fixture.harness.mail.local_outcome(peer, item).await?;
            if marks.max_handoff_seq == Some(1) && matches!(outcome, Some(RecipientOutcome::Held)) {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let store = fixture
        .harness
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("local project file")?;
    let reference: MailItemRef = item.as_uuid().to_string().try_into()?;
    let mut read = store.snapshot().await?;
    let head = read.mail_handoff_head().await?;
    let feed_before = read.high_water();
    let handoff = read
        .mail_handoff(&reference)
        .await?
        .ok_or("local frozen handoff")?;
    read.finish().await?;
    let MailHandoffItem::Delivery(original) = handoff.item else {
        return Err("expected original local handoff".into());
    };
    let cancel = DeliveryCancel::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        original.sender().clone(),
        original.recipient().clone(),
        None,
        CancelBody { target: item },
    )?;
    // This existing receiver hook gates only an actual handoff_frontier upsert.
    let (entered, release) = fixture
        .harness
        .central
        .notifications
        .hold_next_frontier()
        .await?;

    // Act: the public internal producer makes the real local SDK outcome terminal.
    fixture
        .harness
        .mail
        .mail
        .cancel_local(
            cancel,
            collaboration_mail_protocol::CancelCause::BoardDerived {},
        )
        .await
        .map_err(|error| io::Error::other(format!("local SDK cancellation: {error:?}")))?;
    let reached = tokio::time::timeout(BOUND, entered).await;
    if !matches!(reached, Ok(Ok(()))) {
        let _ = release.send(());
        drop(store);
        fixture.close().await?;
        return Err("terminal outcome did not produce an actual frontier copy".into());
    }
    let before = async {
        let marks = fixture
            .harness
            .projects
            .acknowledged_mail_changes(&fixture.project)
            .await?;
        let progress = fixture
            .harness
            .mail
            .owner
            .mail_retired_through(&fixture.project)
            .await?;
        let outcome = fixture.harness.mail.local_outcome(peer, item).await?;
        let mut read = store.snapshot().await?;
        let frontier = read.mail_settled_through().await?;
        let feed = read.high_water();
        read.finish().await?;
        Ok::<_, TestError>((marks, progress, outcome, frontier, feed))
    }
    .await;
    release
        .send(())
        .map_err(|_| io::Error::other("frontier receiver gate closed"))?;
    // No board write, Welcome, readiness hint or manual publisher prompt follows.
    let retired = tokio::time::timeout(BOUND, async {
        loop {
            if fixture
                .harness
                .mail
                .owner
                .mail_retired_through(&fixture.project)
                .await?
                == head
            {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let marks_after = fixture
        .harness
        .projects
        .acknowledged_mail_changes(&fixture.project)
        .await?;
    let outcome_after = fixture.harness.mail.local_outcome(peer, item).await?;
    let push = item.as_uuid().to_string().try_into()?;
    let shown = fixture.show_local_push(&push).await?;
    let seed = NotificationSeed {
        project: fixture.project.clone(),
        item,
        epoch: fixture.seed_epoch,
    };
    drop(store);
    let central = fixture.close_keeping_central().await?;
    let reopened = Box::pin(ProjectNotificationFixture::resume(
        directory.path(),
        central,
        seed,
    ))
    .await?;
    let restored_marks = reopened
        .harness
        .projects
        .acknowledged_mail_changes(&reopened.project)
        .await?;
    let restored_progress = reopened
        .harness
        .mail
        .owner
        .mail_retired_through(&reopened.project)
        .await?;
    let restored_outcome = reopened.harness.mail.local_outcome(peer, item).await?;
    let restored_show = reopened.show_local_push(&push).await?;
    reopened.close().await?;

    // Assert only after every gate and physical owner has drained.
    let (marks_before, progress_before, outcome_before, frontier, feed) = before?;
    assert_eq!(frontier, head);
    assert_eq!(
        feed, feed_before,
        "frontier advancement wrote a board feed record"
    );
    assert!(
        marks_before.settled_through.is_none(),
        "held frontier was acknowledged early"
    );
    assert_eq!(
        progress_before,
        HandoffSeq::ZERO,
        "unacknowledged custody was retired"
    );
    assert!(matches!(
        outcome_before,
        Some(RecipientOutcome::Prevented { .. })
    ));
    assert!(
        retired.is_ok(),
        "real frontier ACK did not prompt retirement"
    );
    retired??;
    assert_eq!(marks_after.settled_through, Some(head.get()));
    assert!(outcome_after.is_none(), "retired local SDK custody remains");
    assert_eq!(restored_marks, marks_after);
    assert_eq!(restored_progress, head);
    assert!(restored_outcome.is_none());
    assert_eq!(restored_show.record.push_id, shown.record.push_id);
    assert_eq!(
        restored_show.record.delivery_state,
        shown.record.delivery_state
    );
    assert_eq!(restored_show.link, shown.link);
    Ok(())
}
