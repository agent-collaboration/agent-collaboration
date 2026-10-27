//! SIGKILL after the real frontier ACK and SDK deletion, before local progress commits.
use super::*;
use super::super::notification_owner::ProjectNotificationFixture;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::NotificationSeed;
use collaboration_service::ProjectReplicationPosition;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, RecipientOutcome};
use message_board::{FeedCursor, HandoffSeq, MailItemRef};
use message_board_storage::MailHandoffItem;

const REJECT_PROGRESS: &str = "CREATE TEMP TRIGGER reject_retirement_progress BEFORE INSERT ON mail_retirement_progress BEGIN SELECT RAISE(ABORT,'retirement checkpoint fault'); END";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetirementCheckpoint {
    seed: NotificationSeed,
    receiver: fleet_sync_protocol::ReceiverId,
    through: HandoffSeq,
    feed: FeedCursor,
    frozen: serde_json::Value,
    shown: collaboration_protocol::PushRecordShowResult,
}

#[tokio::test]
async fn acknowledged_sdk_retirement_retries_absent_custody_after_sigkill_before_progress()
-> Result<(), TestError> {
    // Arrange: the child can report ready only after the real ACK and deletion.
    let (directory, state) = kill_child(CrashKind::FrontierRetirement).await?;
    let checkpoint: RetirementCheckpoint =
        serde_json::from_slice(&std::fs::read(directory.path().join("retirement.json"))?)?;
    let item = checkpoint.seed.item;
    assert_eq!(state.item, Some(item));
    let mut central = TestFleet::start_mail_with_receiver(
        &directory.path().join("central"),
        Some(state.peer),
        Some(checkpoint.receiver),
    )
    .await?;
    central.release()?;
    let mut reopened = Box::pin(ProjectNotificationFixture::resume(
        directory.path(),
        central,
        checkpoint.seed,
    ))
    .await?;
    let marks = reopened
        .harness
        .projects
        .acknowledged_mail_changes(&reopened.project)
        .await?;
    assert_eq!(
        marks.settled_through,
        Some(checkpoint.through.get()),
        "actual frontier ACK did not survive SIGKILL"
    );
    assert_eq!(
        reopened
            .harness
            .mail
            .owner
            .mail_retired_through(&reopened.project)
            .await?,
        HandoffSeq::ZERO
    );
    assert!(
        reopened
            .harness
            .mail
            .local_outcome(state.peer, item)
            .await?
            .is_none(),
        "SDK deletion did not survive SIGKILL"
    );
    let push = item.as_uuid().to_string().try_into()?;
    assert_eq!(
        serde_json::to_value(reopened.show_local_push(&push).await?)?,
        serde_json::to_value(&checkpoint.shown)?
    );
    let store = reopened
        .harness
        .registry
        .existing_project(&reopened.project)
        .await?
        .ok_or("reopened project")?;
    let mut read = store.snapshot().await?;
    let reference: MailItemRef = item.as_uuid().to_string().try_into()?;
    assert_eq!(
        handoff_snapshot(
            read.mail_handoff(&reference)
                .await?
                .ok_or("frozen handoff")?
        )?,
        checkpoint.frozen
    );
    assert_eq!(read.high_water(), checkpoint.feed);
    assert_eq!(read.mail_settled_through().await?, checkpoint.through);
    read.finish().await?;
    // The fault belongs to the killed connection. Normal Welcome repairs progress.
    reopened.start().await?;
    let recovered = tokio::time::timeout(BOUND, async {
        loop {
            if reopened
                .harness
                .mail
                .owner
                .mail_retired_through(&reopened.project)
                .await?
                == checkpoint.through
            {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let final_show = reopened.show_local_push(&push).await?;
    let final_outcome = reopened
        .harness
        .mail
        .local_outcome(state.peer, item)
        .await?;
    let mut read = store.snapshot().await?;
    let final_head = read.mail_handoff_head().await?;
    let final_feed = read.high_water();
    let final_frozen = handoff_snapshot(
        read.mail_handoff(&reference)
            .await?
            .ok_or("final frozen handoff")?,
    )?;
    read.finish().await?;
    drop(store);
    reopened.close().await?;
    // Assert after all real owners drain, including on a missing-Absent mutant.
    assert!(
        recovered.is_ok(),
        "absent native custody stranded acknowledged retirement progress after SIGKILL"
    );
    recovered??;
    assert!(
        final_outcome.is_none(),
        "retirement replay recreated SDK custody"
    );
    assert_eq!(
        serde_json::to_value(final_show)?,
        serde_json::to_value(checkpoint.shown)?
    );
    assert_eq!(
        final_head, checkpoint.through,
        "retirement recovery allocated a duplicate handoff"
    );
    assert_eq!(final_feed, checkpoint.feed);
    assert_eq!(final_frozen, checkpoint.frozen);
    Ok(())
}

pub(super) async fn retirement_checkpoint_child(base: &Path) -> Result<(), TestError> {
    let mut fixture = Box::pin(ProjectNotificationFixture::open_local(base)).await?;
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
            if marks.max_handoff_seq == Some(1)
                && matches!(
                    fixture.harness.mail.local_outcome(peer, item).await?,
                    Some(RecipientOutcome::Held)
                )
            {
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
        .ok_or("seed project")?;
    let reference: MailItemRef = item.as_uuid().to_string().try_into()?;
    let mut read = store.snapshot().await?;
    let handoff = read
        .mail_handoff(&reference)
        .await?
        .ok_or("frozen original")?;
    let through = read.mail_handoff_head().await?;
    let feed = read.high_water();
    let frozen = serde_json::to_value((&handoff.sequence, &handoff.item))?;
    read.finish().await?;
    let MailHandoffItem::Delivery(original) = handoff.item else {
        return Err("expected original".into());
    };
    let control = DeliveryCancel::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        original.sender().clone(),
        original.recipient().clone(),
        None,
        CancelBody { target: item },
    )?;
    fixture
        .harness
        .mail
        .retirement_progress_sql(REJECT_PROGRESS)
        .await?;
    fixture
        .harness
        .mail
        .mail
        .cancel_local(
            control,
            collaboration_mail_protocol::CancelCause::BoardDerived {},
        )
        .await
        .map_err(|e| io::Error::other(format!("real local cancellation: {e:?}")))?;
    // No fabricated mark or manual publisher prompt: Host handles the real ACK.
    tokio::time::timeout(BOUND, async {
        loop {
            let marks = fixture
                .harness
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?;
            if marks.settled_through == Some(through.get())
                && fixture
                    .harness
                    .mail
                    .local_outcome(peer, item)
                    .await?
                    .is_none()
            {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert_eq!(
        fixture
            .harness
            .mail
            .owner
            .mail_retired_through(&fixture.project)
            .await?,
        HandoffSeq::ZERO
    );
    assert!(
        fixture
            .harness
            .mail
            .owner
            .advance_mail_retired_through(&fixture.project, through)
            .await
            .is_err(),
        "the connection-local fault must reject the actual progress write"
    );
    let push = item.as_uuid().to_string().try_into()?;
    let checkpoint = RetirementCheckpoint {
        receiver: fixture.harness.central.receiver,
        seed: NotificationSeed {
            project: fixture.project.clone(),
            item,
            epoch: fixture.seed_epoch,
        },
        through,
        feed,
        frozen,
        shown: fixture.show_local_push(&push).await?,
    };
    std::fs::write(
        base.join("retirement.json"),
        serde_json::to_vec(&checkpoint)?,
    )?;
    let state = CrashState {
        peer,
        original_frames: Vec::new(),
        published_applied: false,
        item: Some(item),
        status_acknowledged: 0,
    };
    std::fs::write(base.join("state.json"), serde_json::to_vec(&state)?)?;
    let mut ready = UnixStream::connect(base.join("ready.sock")).await?;
    ready.write_all(b"R").await?;
    std::future::pending::<Result<(), TestError>>().await
}

fn handoff_snapshot(
    handoff: message_board_storage::MailHandoff,
) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value((handoff.sequence, handoff.item))
}
