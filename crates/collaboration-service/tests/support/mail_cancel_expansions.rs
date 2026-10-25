//! Valid frozen follow/control rows for native capture and snapshot contract tests.
type TestError = Box<dyn std::error::Error + Send + Sync>;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{
    CancelBody, DeliveryCancel, DeliveryEnvelope, DeliveryItem, ItemId, PeerId, Recipient, SyncTime,
};
use message_board::*;
use message_board_storage::{
    MailCancelExpansion, MailCancelGroup, MailCancelGroupCause, MailHandoffItem,
    ProjectWriteTransaction,
};

pub struct FrozenFollowCancels {
    pub targets: Vec<MailItemRef>,
    pub group: MailItemRef,
    sender: MailSender,
    recipient: Recipient<SessionRef>,
}

impl FrozenFollowCancels {
    #[allow(dead_code)] // Native capture drivers use the default local recipient.
    pub async fn arrange(
        write: &mut ProjectWriteTransaction,
        project: &ProjectId,
    ) -> Result<Self, TestError> {
        let session = SessionRef {
            endpoint: SessionEndpointRef {
                service_id: write.origin().installation,
                endpoint_id: "codex-local".to_owned().try_into()?,
            },
            session_id: "capture-control-recipient".to_owned().try_into()?,
        };
        Self::arrange_for_recipient(
            write,
            project,
            Recipient {
                peer: PeerId::new(uuid::Uuid::now_v7())?,
                address: session,
            },
        )
        .await
    }

    pub async fn arrange_for_recipient(
        write: &mut ProjectWriteTransaction,
        project: &ProjectId,
        recipient: Recipient<SessionRef>,
    ) -> Result<Self, TestError> {
        let session = recipient.address.clone();
        let sender = MailSender::Project {
            project: project.clone(),
        };
        let membership: collaboration_mail_protocol::MembershipId =
            RecordId::generate().as_str().to_owned().try_into()?;
        let activated_at = write.high_water().await?;
        let generation = SubscriptionGeneration::new(1)?;
        let mut targets = Vec::new();
        for _ in 0..3 {
            let id = ItemId::new(uuid::Uuid::now_v7())?;
            let thread = ThreadId::generate();
            let cursor = FeedCursor {
                position: activated_at.position.successor()?,
                ..activated_at
            };
            let body = BoardDeliveryBody::Notification(NotificationBody {
                excerpt: "captured follow".to_owned().try_into()?,
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
                    policy: SubscriptionPolicy::defaults_for(&Identity::Session {
                        session: session.clone(),
                    }),
                    draining: false,
                },
            });
            let item = MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(DeliveryItem::<
                BoardDeliveryTypes,
            >::new(
                id,
                sender.clone(),
                recipient.clone(),
                None,
                body,
            )?)));
            write.append_mail_handoff(&item).await?;
            targets.push(item.id()?);
        }
        let group: MailItemRef = uuid::Uuid::now_v7().to_string().try_into()?;
        write
            .append_mail_handoff(&MailHandoffItem::CancelGroup(MailCancelGroup {
                id: group.clone(),
                membership,
                activated_at,
                generation,
                cause: MailCancelGroupCause::MembershipEnded {
                    reason: EndReason::Cancelled,
                },
            }))
            .await?;
        Ok(Self {
            targets,
            group,
            sender,
            recipient,
        })
    }

    #[allow(dead_code)] // Capture drivers also check a concrete frozen deadline.
    pub fn child(&self, target: &MailItemRef) -> Result<MailCancelExpansion, TestError> {
        self.child_with_deadline(target, Some(SyncTime::from_unix_millis(900_000)))
    }

    pub fn child_with_deadline(
        &self,
        target: &MailItemRef,
        deadline: Option<SyncTime>,
    ) -> Result<MailCancelExpansion, TestError> {
        Ok(MailCancelExpansion {
            group_handoff: self.group.clone(),
            target_item: target.clone(),
            envelope: Box::new(DeliveryEnvelope::Cancel(DeliveryCancel::new(
                ItemId::new(uuid::Uuid::now_v7())?,
                self.sender.clone(),
                self.recipient.clone(),
                deadline,
                CancelBody {
                    target: ItemId::new(uuid::Uuid::parse_str(target.as_str())?)?,
                },
            )?)),
        })
    }
}
