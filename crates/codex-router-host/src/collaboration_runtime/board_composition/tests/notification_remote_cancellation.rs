//! Actual capture, PartStaged and physical reopen gate the frozen child on its own prefix.
use super::*;
use super::notification_owner::ProjectNotificationFixture;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::NotificationSeed;
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange};
use collaboration_service::ProjectReplicationPosition;
use fleet_sync_delivery::OutboxStatusState;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, Recipient};
use fleet_sync_writer::{ReplicationEvents, ReplicationSender, SenderOptions};
use message_board::*;
use message_board_storage::MailHandoffItem;
use std::num::NonZeroUsize;
#[path = "../../../../../collaboration-service/tests/support/mail_cancel_expansions.rs"]
mod frozen_rows;
use super::split_transport;

#[tokio::test]
async fn remote_children_wait_for_their_own_final_ack_and_reopen_reuses_every_frozen_envelope()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut fixture = ProjectNotificationFixture::open(directory.path()).await?;
    let seed = NotificationSeed {
        project: fixture.project.clone(),
        item: fixture.seed_item,
        epoch: fixture.seed_epoch,
    };
    let writer = fixture.harness.projects.ensure(&seed.project).await?;
    let local_service: ServiceId = "00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()?;
    let origin = ActorOrigin {
        installation: local_service.clone(),
        machine: "notice fixture".to_owned().try_into()?,
    };
    let mut write = writer.store.begin_admission(origin).await?;
    // Frozen producer rows are the fixture input. Capture, eligibility, native SDK
    // admission and the consumer wire below stay real; this is not allocation-policy proof.
    let frozen = frozen_rows::FrozenFollowCancels::arrange_for_recipient(
        &mut write,
        &seed.project,
        Recipient {
            peer: PeerId::new(durable_steps::Uuid::now_v7())?,
            address: SessionRef {
                endpoint: SessionEndpointRef {
                    service_id: "00000000-0000-4000-8000-000000000099"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "remote-cancel-recipient".to_owned().try_into()?,
            },
        },
    )
    .await?;
    let barrier = ItemId::new(durable_steps::Uuid::now_v7())?;
    let position = write.high_water().await?;
    let root = ThreadId::generate();
    let local_recipient = SessionRef {
        endpoint: SessionEndpointRef {
            service_id: local_service,
            endpoint_id: "codex-local".to_owned().try_into()?,
        },
        session_id: "cancel-prefix-pass-barrier".to_owned().try_into()?,
    };
    let cursor = FeedCursor {
        position: position.position.successor()?,
        ..position
    };
    write
        .append_mail_handoff(&MailHandoffItem::Delivery(Box::new(
            DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
                barrier,
                MailSender::Project {
                    project: seed.project.clone(),
                },
                Recipient {
                    peer: fixture.harness.peer(),
                    address: local_recipient.clone(),
                },
                None,
                BoardDeliveryBody::Notification(NotificationBody {
                    excerpt: "later local publication witnesses the group pass"
                        .to_owned()
                        .try_into()?,
                    pointer: RecordPointer {
                        project: seed.project.clone(),
                        subject: root.clone().into(),
                        position: cursor,
                    },
                    source: NotificationSource::FollowBatch {
                        membership: RecordId::generate().as_str().to_owned().try_into()?,
                        activated_at: position,
                        generation: SubscriptionGeneration::new(1)?,
                        roots: vec![RootRange::new(root, cursor, cursor, 1)?],
                        policy: SubscriptionPolicy::defaults_for(&Identity::Session {
                            session: local_recipient,
                        }),
                        draining: false,
                    },
                }),
            )?),
        )))
        .await?;
    write.commit().await?;
    let database = fixture.harness.registry.database(&seed.project).await?;
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        writer.runtime.transport(),
        SenderOptions::default(),
    );
    // replicate_once prepares one committed transaction, not the entire tail.
    let parent = tokio::time::timeout(BOUND, async {
        loop {
            let events = sender
                .replicate_once(fixture.harness.central.receiver)
                .await?;
            let marks = fixture
                .harness
                .projects
                .acknowledged_mail_changes(&seed.project)
                .await?;
            if marks.max_handoff_seq.is_some_and(|through| through >= 6) {
                return Ok::<_, TestError>(marks);
            }
            if !events
                .iter()
                .any(|event| matches!(event, ReplicationEvents::Acknowledged { .. }))
            {
                return Err("native sender made no progress toward the parent prefix".into());
            }
        }
    })
    .await??;
    assert_eq!(parent.max_handoff_seq, Some(6));
    assert_eq!(parent.max_expansion_seq, None);
    drop(sender);

    // Batch three immutable associations to force the real SDK's transaction split.
    let mut write = writer
        .store
        .begin_admission(ActorOrigin {
            installation: "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()?,
            machine: "notice fixture".to_owned().try_into()?,
        })
        .await?;
    let mut expected = Vec::new();
    for target in &frozen.targets {
        let child = frozen.child_with_deadline(target, None)?;
        expected.push((child.envelope.id(), serde_json::to_vec(&child.envelope)?));
        write.append_mail_cancel_expansion(&child).await?;
    }
    write.commit().await?;
    let (transport, staged, release) =
        split_transport::SplitTransport::new(writer.runtime.transport());
    let sender = Arc::new(ReplicationSender::new(
        database.clone(),
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
    assert!(
        transport.held_transaction_wrote_expansion().await,
        "held PartStaged was not the child transaction"
    );
    let staged_marks = fixture
        .harness
        .projects
        .acknowledged_mail_changes(&seed.project)
        .await?;
    assert_eq!(staged_marks.max_handoff_seq, Some(6));
    assert_eq!(staged_marks.max_expansion_seq, None);
    fixture.harness.start_mail();
    fixture
        .publisher
        .queue_project(seed.project.clone())
        .await?;
    // This later bare handoff is visited only after the group in the same pass;
    // its SDK outcome prevents an early assertion from passing vacuously.
    tokio::time::timeout(BOUND, async {
        loop {
            if fixture
                .harness
                .mail
                .local_outcome(fixture.harness.peer(), barrier)
                .await?
                .is_some()
            {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let mut early_admission = false;
    for (id, _) in &expected {
        early_admission |= fixture
            .harness
            .mail
            .owner
            .mail_submission_epoch(&id.as_uuid().to_string().try_into()?)
            .await?
            .is_some();
        early_admission |= !matches!(
            fixture.harness.mail.outbox_status(*id).await?.state,
            OutboxStatusState::Unknown
        );
        early_admission |= fixture
            .harness
            .central
            .notifications
            .has_submission(*id)
            .await;
    }
    if early_admission {
        release
            .send(())
            .map_err(|_| io::Error::other("child split closed"))?;
        tokio::time::timeout(BOUND, copying).await???;
        drop(sender);
        drop(transport);
        drop(database);
        drop(writer);
        fixture.close().await?;
        return Err("acknowledged parent or PartStaged admitted an unacknowledged child".into());
    }
    for target in &frozen.targets {
        let id = ItemId::new(durable_steps::Uuid::parse_str(target.as_str())?)?;
        tokio::time::timeout(
            BOUND,
            fixture.harness.central.notifications.wait_submission(id),
        )
        .await?;
    }
    release
        .send(())
        .map_err(|_| io::Error::other("child split closed"))?;
    let events = tokio::time::timeout(BOUND, copying).await???;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ReplicationEvents::Acknowledged { .. }))
    );
    let acknowledged = fixture
        .harness
        .projects
        .acknowledged_mail_changes(&seed.project)
        .await?;
    assert_eq!(acknowledged.max_expansion_seq, Some(3));
    drop(sender);
    drop(transport);
    drop(database);
    drop(writer);
    let central = fixture.close_keeping_central().await?;
    let mut reopened = ProjectNotificationFixture::resume(directory.path(), central, seed).await?;
    assert_eq!(
        reopened
            .harness
            .projects
            .acknowledged_mail_changes(&reopened.project)
            .await?,
        acknowledged
    );
    reopened.harness.start_mail();
    reopened
        .publisher
        .queue_project(reopened.project.clone())
        .await?;
    let mut actual = Vec::new();
    for (id, encoded) in &expected {
        let epoch = tokio::time::timeout(
            BOUND,
            reopened.harness.central.notifications.wait_submission(*id),
        )
        .await?;
        let recorded = reopened
            .harness
            .mail
            .owner
            .mail_submission_epoch(&id.as_uuid().to_string().try_into()?)
            .await?
            .ok_or("child epoch missing")?;
        let envelopes = reopened
            .harness
            .central
            .notifications
            .submitted_envelopes(*id)
            .await;
        actual.push((
            epoch,
            recorded.epoch,
            !envelopes.is_empty() && envelopes.iter().all(|envelope| envelope == encoded),
        ));
    }
    let epoch = reopened.seed_epoch;
    reopened.close().await?;
    assert!(
        !early_admission,
        "acknowledged parent or PartStaged admitted an unacknowledged child"
    );
    assert_eq!(actual.len(), 3);
    assert!(
        actual.iter().all(|(wire, saved, same)| *wire
            == Some(fleet_sync_protocol::Epoch::new(epoch.get()).unwrap())
            && *saved == epoch
            && *same),
        "own final ACK/reopen changed a child identity, epoch or frozen envelope: {actual:?}"
    );
    Ok(())
}
