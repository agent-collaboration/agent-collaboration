//! Staged parts cannot release mail; final acknowledged marks survive physical reopen.
use super::*;
use super::notification_owner::ProjectNotificationFixture;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::NotificationSeed;
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation};
use collaboration_contracts::ProjectReplicationPosition;
use fleet_sync_writer::{ReplicationEvents, ReplicationSender, SenderOptions};
use std::num::NonZeroUsize;
use super::split_transport::SplitTransport;

#[tokio::test]
async fn staged_handoff_parts_wait_for_the_final_acknowledgement_and_reopen_keeps_the_mark()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut fixture = Box::pin(ProjectNotificationFixture::open(directory.path())).await?;
    let seed = NotificationSeed {
        project: fixture.project.clone(),
        item: fixture.seed_item,
        epoch: fixture.seed_epoch,
    };
    let item: collaboration_protocol::MailItemId = seed.item.as_uuid().to_string().try_into()?;
    assert!(!*fixture.harness.welcomed.borrow());
    // The real SDK pump is welcomed, but Host activation is gated, so its
    // prepared writer has no default sender racing this forced split.
    let writer = fixture.harness.projects.ensure(&seed.project).await?;
    let (transport, staged, release) = SplitTransport::new(writer.runtime.transport());
    let sender = Arc::new(ReplicationSender::new(
        fixture.harness.registry.database(&seed.project).await?,
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        transport.clone(),
        SenderOptions {
            max_part_items: NonZeroUsize::MIN,
            ..SenderOptions::default()
        },
    ));
    let receiver = fixture.harness.central.receiver;
    let copying = {
        let sender = sender.clone();
        tokio::spawn(async move { sender.replicate_once(receiver).await })
    };
    tokio::time::timeout(BOUND, staged).await??;
    assert!(transport.staged_answers() > 0);
    let staged_marks = fixture
        .harness
        .projects
        .acknowledged_mail_changes(&seed.project)
        .await?;
    assert!(
        staged_marks
            .feed
            .is_some_and(|cursor| cursor.position.get() > 0)
    );
    assert!(staged_marks.max_handoff_seq.is_none());
    fixture.harness.start_mail();
    fixture
        .publisher
        .queue_project(seed.project.clone())
        .await?;
    tokio::time::timeout(BOUND, async {
        loop {
            if fixture
                .harness
                .mail
                .owner
                .intents_scanned_through(&seed.project)
                .await?
                .is_some()
            {
                return Ok::<_, interaction_storage::StorageError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    // Outgoing custody and this epoch association commit together, before a
    // consumed prefix can be saved. This catches a feed-as-handoff substitute.
    let early_epoch = fixture
        .harness
        .mail
        .owner
        .mail_submission_epoch(&item)
        .await?;
    let early_wire = fixture
        .harness
        .central
        .notifications
        .has_submission(seed.item)
        .await;
    release
        .send(())
        .map_err(|_| io::Error::other("split sender closed"))?;
    let events = tokio::time::timeout(BOUND, copying).await???;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ReplicationEvents::Acknowledged { .. }))
    );
    let final_marks = fixture
        .harness
        .projects
        .acknowledged_mail_changes(&seed.project)
        .await?;
    let staged_handoff = transport.held_transaction_wrote_handoff().await;
    drop(sender);
    drop(transport);
    drop(writer);
    let central = fixture.close_keeping_central().await?;
    // Every original local file owner is closed. Reconstruct the port from a
    // newly opened native registry, before new replication or Host Welcome.
    let mut reopened = Box::pin(ProjectNotificationFixture::resume(
        directory.path(),
        central,
        seed,
    ))
    .await?;
    assert!(!*reopened.harness.welcomed.borrow());
    let restored_marks = reopened
        .harness
        .projects
        .acknowledged_mail_changes(&reopened.project)
        .await?;
    reopened.harness.start_mail();
    reopened
        .publisher
        .queue_project(reopened.project.clone())
        .await?;
    let published = tokio::time::timeout(
        Duration::from_secs(10),
        reopened
            .harness
            .central
            .notifications
            .wait_submission(reopened.seed_item),
    )
    .await;
    let restored_epoch = reopened
        .harness
        .mail
        .owner
        .mail_submission_epoch(&item)
        .await?;
    let epoch = reopened.seed_epoch;
    reopened.close().await?;
    assert!(
        staged_handoff,
        "the held PartStaged did not belong to the handoff transaction"
    );
    assert!(
        early_epoch.is_none(),
        "a staged part admitted remote custody before the handoff acknowledgement"
    );
    assert!(!early_wire, "a staged part published remote mail");
    assert_eq!(final_marks.max_handoff_seq, Some(1));
    assert_eq!(
        restored_marks, final_marks,
        "physical reopen lost the acknowledged capture mapping"
    );
    assert!(
        published.is_ok(),
        "reopened acknowledged handoff did not publish"
    );
    assert_eq!(
        published?,
        Some(fleet_sync_protocol::Epoch::new(epoch.get())?)
    );
    assert_eq!(
        restored_epoch
            .ok_or("reopened outgoing epoch missing")?
            .epoch,
        epoch
    );
    Ok(())
}
