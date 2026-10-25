//! Real membership changes freeze one original and one inherited cancel association.
use super::*;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryEnvelope, DeliveryItem, Recipient};
use message_board_storage::{MailCancelExpansion, MailCancelGroup, MailHandoffItem};

pub(in crate::collaboration_runtime::board_composition::tests::messaging_rule_cases) struct FollowSeed
{
    pub item: DeliveryItem<BoardDeliveryTypes>,
    pub membership: SubscriptionMembership,
    pub root: ThreadId,
}
impl RuleFixture {
    fn follow_reader(
        &self,
        local: bool,
    ) -> Result<(Identity, PeerId), Box<dyn std::error::Error + Send + Sync>> {
        let (service, peer) = if local {
            (
                origin()?.installation,
                self.holder.central.session_inputs()?.peer,
            )
        } else {
            (
                RECIPIENT.to_owned().try_into()?,
                PeerId::new(self.recipient.peer)?,
            )
        };
        Ok((
            Identity::Session {
                session: SessionRef {
                    endpoint: SessionEndpointRef {
                        service_id: service,
                        endpoint_id: "codex-local".to_owned().try_into()?,
                    },
                    session_id: "cancel rule reader".to_owned().try_into()?,
                },
            },
            peer,
        ))
    }
    async fn submit_as(&self, actor: Identity, operation: BoardOperation) -> TestResult {
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: self.project.clone(),
            actor,
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        };
        let result = self.holder.admission.submit(&request).await?;
        assert!(
            matches!(
                result.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "membership fixture admission failed"
        );
        Ok(())
    }
    pub async fn seed_follow(
        &self,
        local: bool,
    ) -> Result<FollowSeed, Box<dyn std::error::Error + Send + Sync>> {
        let post = PostId::generate();
        self.write(serde_json::from_value(
            json!({"kind":"threadCreate","postId":post,
            "about":{"kind":"project","subject":{"line":"cancel rule"}},"text":"follow root",
            "references":[],"participation":"participant","watch":false,"addressees":[]}),
        )?)
        .await?;
        let root = ThreadId::try_from(post.as_str().to_owned())?;
        let (reader, peer) = self.follow_reader(local)?;
        self.submit_as(
            reader.clone(),
            BoardOperation::ThreadJoin {
                thread_id: root.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: Some(SubscriptionMode::Deliver),
                when_idle: Some(WhenIdle::Hold),
                replace: None,
                note: None,
            },
        )
        .await?;
        let store = self
            .holder
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("holder store")?;
        let mut read = store.snapshot().await?;
        let membership = read
            .records(RecordKind::SubscriptionMembership)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::SubscriptionMembership(member) if member.reader() == &reader => {
                    Some(member)
                }
                _ => None,
            })
            .ok_or("real joined membership")?;
        read.finish().await?;
        let activity = PostId::generate();
        self.write(BoardOperation::ThreadPost {
            post_id: activity.clone(),
            thread_id: root.clone(),
            text: "one held follow before takeover".to_owned().try_into()?,
            references: Vec::new(),
            addressees: Vec::new(),
        })
        .await?;
        let mut read = store.snapshot().await?;
        let Some(Record::Post(post)) = read.record(&activity.into()).await? else {
            return Err("follow activity missing".into());
        };
        let through = post
            .envelope
            .position
            .project_cursor()
            .ok_or("follow activity cursor")?;
        read.finish().await?;
        let Identity::Session { session } = reader else {
            return Err("session reader".into());
        };
        let item = self.build_follow_item(&membership, &root, session, peer, through)?;
        let mut write = store.begin_admission(origin()?).await?;
        let original = serde_json::to_value(&item)?;
        write
            .append_mail_handoff(&MailHandoffItem::Delivery(Box::new(
                DeliveryEnvelope::Item(item),
            )))
            .await?;
        write.commit().await?;
        Ok(FollowSeed {
            item: serde_json::from_value(original)?,
            membership,
            root,
        })
    }
    fn build_follow_item(
        &self,
        membership: &SubscriptionMembership,
        root: &ThreadId,
        session: SessionRef,
        peer: PeerId,
        through: FeedCursor,
    ) -> Result<DeliveryItem<BoardDeliveryTypes>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(DeliveryItem::new(
            ItemId::try_from("019f0000-0000-7000-8000-000000081081".to_owned())?,
            MailSender::Project {
                project: self.project.clone(),
            },
            Recipient {
                peer,
                address: session,
            },
            None,
            BoardDeliveryBody::Notification(NotificationBody {
                excerpt: "one held follow before takeover".to_owned().try_into()?,
                pointer: RecordPointer {
                    project: self.project.clone(),
                    subject: root.clone().into(),
                    position: through,
                },
                source: NotificationSource::FollowBatch {
                    membership: membership.envelope.id.as_str().to_owned().try_into()?,
                    activated_at: membership.activated_at(),
                    generation: membership.generation(),
                    roots: vec![RootRange::new(root.clone(), through, through, 1)?],
                    policy: membership.policy().clone(),
                    draining: false,
                },
            }),
        )?)
    }
    pub async fn end_follow(
        &self,
        seed: &FollowSeed,
    ) -> Result<MailCancelGroup, Box<dyn std::error::Error + Send + Sync>> {
        self.submit_as(
            seed.membership.reader().clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: SubscriptionScope::thread(seed.root.clone()),
            },
        )
        .await?;
        let store = self
            .holder
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("holder store")?;
        let mut read = store.snapshot().await?;
        let group = read
            .mail_handoff_page(HandoffSeq::ZERO, 100)
            .await?
            .into_iter()
            .find_map(|handoff| match handoff.item {
                MailHandoffItem::CancelGroup(group)
                    if group.membership.as_str() == seed.membership.envelope.id.as_str() =>
                {
                    Some(group)
                }
                _ => None,
            })
            .ok_or("membership end frozen group")?;
        read.finish().await?;
        Ok(group)
    }
    pub async fn freeze_cancel(
        &self,
        seed: &FollowSeed,
        group: &MailCancelGroup,
    ) -> Result<DeliveryEnvelope<BoardDeliveryTypes>, Box<dyn std::error::Error + Send + Sync>>
    {
        let envelope = DeliveryEnvelope::Cancel(DeliveryCancel::new(
            ItemId::try_from("019f0000-0000-7000-8000-000000081082".to_owned())?,
            seed.item.sender().clone(),
            seed.item.recipient().clone(),
            None,
            CancelBody {
                target: seed.item.id(),
            },
        )?);
        let store = self
            .holder
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("holder store")?;
        let mut write = store.begin_admission(origin()?).await?;
        let original = serde_json::to_value(&envelope)?;
        write
            .append_mail_cancel_expansion(&MailCancelExpansion {
                group_handoff: group.id.clone(),
                target_item: seed.item.id().as_uuid().to_string().try_into()?,
                envelope: Box::new(envelope),
            })
            .await?;
        write.commit().await?;
        Ok(serde_json::from_value(original)?)
    }
}
