//! Native expansion rows keep child identity, immutable associations and independent order.
use crate::{
    MailCancelExpansion, MailCancelGroup, MailCancelGroupCause, MailHandoffItem,
    ProjectStorageError, ProjectStore, project_test_support::origin,
};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, MembershipId, NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{
    CancelBody, DeliveryCancel, DeliveryEnvelope, DeliveryItem, ItemId, PeerId, Recipient,
};
use message_board::*;
use sqlx::{ConnectOptions, Connection};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct ExpansionFixture {
    project: ProjectId,
    first: MailHandoffItem,
    second: MailHandoffItem,
    group: MailHandoffItem,
}

impl ExpansionFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let project = ProjectId::generate();
        let session = SessionRef {
            endpoint: SessionEndpointRef {
                service_id: origin().installation,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from("expansion-recipient".to_owned())?,
        };
        let recipient = Recipient {
            peer: PeerId::new(uuid::Uuid::now_v7())?,
            address: session.clone(),
        };
        let membership = MembershipId::try_from(RecordId::generate().as_str().to_owned())?;
        let activated_at = FeedCursor {
            epoch: EpochNumber::INITIAL,
            position: FeedPosition::ZERO,
        };
        let generation = SubscriptionGeneration::new(1)?;
        let policy = SubscriptionPolicy::defaults_for(&Identity::Session { session });
        let follow = || -> Result<MailHandoffItem, Box<dyn std::error::Error + Send + Sync>> {
            let cursor = FeedCursor {
                position: FeedPosition::try_from(1)?,
                ..activated_at
            };
            let thread = ThreadId::generate();
            Ok(MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(
                DeliveryItem::<BoardDeliveryTypes>::new(
                    ItemId::new(uuid::Uuid::now_v7())?,
                    MailSender::Project {
                        project: project.clone(),
                    },
                    recipient.clone(),
                    None,
                    BoardDeliveryBody::Notification(NotificationBody {
                        excerpt: "frozen follow".to_owned().try_into()?,
                        pointer: RecordPointer {
                            project: project.clone(),
                            subject: thread.clone().into(),
                            position: cursor,
                        },
                        source: NotificationSource::FollowBatch {
                            membership: membership.clone(),
                            activated_at,
                            generation,
                            roots: vec![RootRange::new(thread, cursor, cursor, 1)?],
                            policy: policy.clone(),
                            draining: false,
                        },
                    }),
                )?,
            ))))
        };
        let first = follow()?;
        let second = follow()?;
        let group = MailHandoffItem::CancelGroup(MailCancelGroup {
            id: uuid::Uuid::now_v7().to_string().try_into()?,
            membership,
            activated_at,
            generation,
            cause: MailCancelGroupCause::MembershipEnded {
                reason: EndReason::Cancelled,
            },
        });
        Ok(Self {
            project,
            first,
            second,
            group,
        })
    }

    fn expansion(
        &self,
        target: &MailHandoffItem,
        id: ItemId,
    ) -> Result<MailCancelExpansion, Box<dyn std::error::Error + Send + Sync>> {
        let MailHandoffItem::Delivery(target) = target else {
            return Err("follow target".into());
        };
        Ok(MailCancelExpansion {
            group_handoff: self.group.id()?,
            target_item: target.id().as_uuid().to_string().try_into()?,
            envelope: Box::new(DeliveryEnvelope::Cancel(DeliveryCancel::new(
                id,
                target.sender().clone(),
                target.recipient().clone(),
                Some(fleet_sync_protocol::SyncTime::from_unix_millis(123_456)),
                CancelBody {
                    target: target.id(),
                },
            )?)),
        })
    }

    async fn arrange(
        &self,
        store: &ProjectStore<board_fleet_sync::BoardProjectSchema>,
    ) -> TestResult {
        let mut write = store.begin_admission(origin()).await?;
        write
            .append_record(crate::project_store_tests::project_record(
                self.project.clone(),
            ))
            .await?;
        write.append_mail_handoff(&self.first).await?;
        write.append_mail_handoff(&self.second).await?;
        write.append_mail_handoff(&self.group).await?;
        write.commit().await?;
        Ok(())
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn expansions_replay_exactly_follow_commit_order_and_reopen_with_the_original_envelope()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = ExpansionFixture::new()?;
    let path = directory.path().join("expansions.sqlite");
    let store = ProjectStore::open(&path, fixture.project.clone()).await?;
    fixture.arrange(&store).await?;
    // Child UUID order deliberately opposes transaction order.
    let first = fixture.expansion(
        &fixture.first,
        ItemId::new(uuid::Uuid::parse_str(
            "01900000-0000-7000-8000-000000000002",
        )?)?,
    )?;
    let second = fixture.expansion(
        &fixture.second,
        ItemId::new(uuid::Uuid::parse_str(
            "01900000-0000-7000-8000-000000000001",
        )?)?,
    )?;
    let expected = serde_json::to_value(&first.envelope)?;
    let mut write = store.begin_admission(origin()).await?;
    let feed = write.high_water().await?;
    assert_eq!(write.append_mail_cancel_expansion(&first).await?.get(), 1);
    assert_eq!(write.append_mail_cancel_expansion(&first).await?.get(), 1);
    write.commit().await?;
    let mut write = store.begin_admission(origin()).await?;
    assert_eq!(write.append_mail_cancel_expansion(&second).await?.get(), 2);
    assert_eq!(
        write.high_water().await?,
        feed,
        "expansions do not invent a board feed event"
    );
    write.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, fixture.project.clone()).await?;
    let mut read = store.snapshot().await?;
    let page = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 1)
        .await?;
    let [original] = page.as_slice() else {
        return Err("one child page".into());
    };
    assert_eq!(original.sequence.get(), 1);
    assert_eq!(original.expansion.id()?, first.id()?);
    assert_eq!(
        serde_json::to_value(&original.expansion.envelope)?,
        expected
    );
    let page = read
        .mail_cancel_expansion_page(original.sequence, 1)
        .await?;
    let [next] = page.as_slice() else {
        return Err("next child page".into());
    };
    assert_eq!(next.sequence.get(), 2);
    assert_eq!(next.expansion.id()?, second.id()?);
    read.finish().await?;
    // A new identity for an existing pair is a conflict, never a successful replay.
    let replacement = fixture.expansion(&fixture.first, ItemId::new(uuid::Uuid::now_v7())?)?;
    let mut write = store.begin_admission(origin()).await?;
    assert!(matches!(
        write.append_mail_cancel_expansion(&replacement).await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    let DeliveryEnvelope::Cancel(original_cancel) = &*first.envelope else {
        return Err("original cancel".into());
    };
    let changed_body = MailCancelExpansion {
        group_handoff: first.group_handoff.clone(),
        target_item: first.target_item.clone(),
        envelope: Box::new(DeliveryEnvelope::Cancel(DeliveryCancel::new(
            original_cancel.id(),
            original_cancel.sender().clone(),
            original_cancel.recipient().clone(),
            Some(fleet_sync_protocol::SyncTime::from_unix_millis(999_999)),
            CancelBody {
                target: original_cancel.target(),
            },
        )?)),
    };
    assert!(
        matches!(
            write.append_mail_cancel_expansion(&changed_body).await,
            Err(ProjectStorageError::InvalidFeed)
        ),
        "same child ID accepted a different frozen deadline/digest"
    );
    write.rollback().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn expansion_requires_an_earlier_follow_with_the_exact_membership_activation_and_generation()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = ExpansionFixture::new()?;
    let store = ProjectStore::open(
        &directory.path().join("relationships.sqlite"),
        fixture.project.clone(),
    )
    .await?;
    fixture.arrange(&store).await?;
    let MailHandoffItem::CancelGroup(group) = &fixture.group else {
        return Err("group".into());
    };
    let mut write = store.begin_admission(origin()).await?;
    for field in ["membership", "activation", "generation"] {
        let mut changed = group.clone();
        changed.id = uuid::Uuid::now_v7().to_string().try_into()?;
        match field {
            "membership" => {
                changed.membership = RecordId::generate().as_str().to_owned().try_into()?
            }
            "activation" => changed.activated_at.position = FeedPosition::try_from(1)?,
            "generation" => changed.generation = changed.generation.next()?,
            _ => return Err("unknown relationship fixture field".into()),
        }
        let parent = changed.id.clone();
        write
            .append_mail_handoff(&MailHandoffItem::CancelGroup(changed))
            .await?;
        let mut child = fixture.expansion(&fixture.first, ItemId::new(uuid::Uuid::now_v7())?)?;
        child.group_handoff = parent;
        assert!(
            matches!(
                write.append_mail_cancel_expansion(&child).await,
                Err(ProjectStorageError::InvalidFeed)
            ),
            "accepted mismatched {field}"
        );
    }
    let MailHandoffItem::Delivery(target) = &fixture.first else {
        return Err("target".into());
    };
    let DeliveryEnvelope::Item(target) = &**target else {
        return Err("target original".into());
    };
    let later = MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(DeliveryItem::<
        BoardDeliveryTypes,
    >::new(
        ItemId::new(uuid::Uuid::now_v7())?,
        target.sender().clone(),
        target.recipient().clone(),
        target.valid_until(),
        target.body().clone(),
    )?)));
    write.append_mail_handoff(&later).await?;
    let child = fixture.expansion(&later, ItemId::new(uuid::Uuid::now_v7())?)?;
    assert!(
        matches!(
            write.append_mail_cancel_expansion(&child).await,
            Err(ProjectStorageError::InvalidFeed)
        ),
        "group accepted a target allocated after its own handoff"
    );
    write.rollback().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn rolled_back_expansion_leaves_no_association_and_does_not_consume_its_sequence()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = ExpansionFixture::new()?;
    let store = ProjectStore::open(
        &directory.path().join("rollback.sqlite"),
        fixture.project.clone(),
    )
    .await?;
    fixture.arrange(&store).await?;
    let child = fixture.expansion(&fixture.first, ItemId::new(uuid::Uuid::now_v7())?)?;
    let mut write = store.begin_admission(origin()).await?;
    assert_eq!(write.append_mail_cancel_expansion(&child).await?.get(), 1);
    write.rollback().await?;
    let mut read = store.snapshot().await?;
    assert!(
        read.mail_cancel_expansion_for_target(&child.group_handoff, &child.target_item)
            .await?
            .is_none()
    );
    read.finish().await?;
    let mut write = store.begin_admission(origin()).await?;
    assert_eq!(write.append_mail_cancel_expansion(&child).await?.get(), 1);
    write.commit().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn invalid_expansion_relationships_and_corrupt_frozen_content_are_refused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = ExpansionFixture::new()?;
    let path = directory.path().join("invalid.sqlite");
    let store = ProjectStore::open(&path, fixture.project.clone()).await?;
    fixture.arrange(&store).await?;
    let child = fixture.expansion(&fixture.first, ItemId::new(uuid::Uuid::now_v7())?)?;
    let mut wrong_target = fixture.expansion(&fixture.first, ItemId::new(uuid::Uuid::now_v7())?)?;
    wrong_target.target_item = fixture.second.id()?;
    let mut write = store.begin_admission(origin()).await?;
    assert!(matches!(
        write.append_mail_cancel_expansion(&wrong_target).await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    write.append_mail_cancel_expansion(&child).await?;
    write.commit().await?;
    store.close().await;
    let original = serde_json::to_value(&child.envelope)?;
    let mut corrupt = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let mut bad_digest = original.clone();
    bad_digest["value"]["validUntil"] = serde_json::json!(654_321);
    for (sequence, target, envelope) in [
        (0_i64, child.target_item.clone(), original.clone()),
        (1, fixture.second.id()?, original.clone()),
        (1, child.target_item.clone(), bad_digest),
    ] {
        sqlx::query("UPDATE mail_cancel_expansions SET expansion_seq=?,target_item=?,envelope=? WHERE cancel_id=?")
            .bind(sequence).bind(target.as_str()).bind(serde_json::to_string(&envelope)?).bind(child.id()?.as_str())
            .execute(&mut corrupt).await?;
        let store = ProjectStore::open(&path, fixture.project.clone()).await?;
        let mut read = store.snapshot().await?;
        assert!(
            matches!(
                read.mail_cancel_expansion_for_target(&child.group_handoff, &target)
                    .await,
                Err(ProjectStorageError::InvalidFeed)
            ),
            "accepted corrupt child seq={sequence}"
        );
        read.finish().await?;
        store.close().await;
    }
    corrupt.close().await?;
    Ok(())
}
