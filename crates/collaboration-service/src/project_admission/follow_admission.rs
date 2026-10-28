//! A reader proposes windows; the locked project owner decides and freezes the allocation.
use super::intent_handoff::IntentRecipientEnrollment;
use super::{ProjectAdmission, ProjectAdmissionError, ProjectWriteTransaction};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, Recipient};
use message_board::{
    EndReason, FeedContent, FeedCursor, FeedRead, HandoffSeq, Identity,
    MAX_SUBSCRIPTION_NOTICE_ROOTS, MailItemRef, Placement, ProjectId, Record, RecordId, RecordKind,
    RecordRef, SubscriptionGeneration, SubscriptionMembership, SubscriptionMode, SubscriptionScope,
    ThreadId,
};
use message_board_storage::MailHandoffItem;

#[derive(Clone)]
pub(crate) struct FollowBatchProposal {
    pub membership: RecordId,
    pub activated_at: FeedCursor,
    pub generation: SubscriptionGeneration,
    pub roots: Vec<RootRange>,
    pub item: ItemId,
    pub enrollment: IntentRecipientEnrollment,
    pub preventions: Vec<crate::notification_publisher::GenerationPreventedFollow>,
}

impl ProjectAdmission {
    pub(crate) async fn allocate_follow_batch(
        &self,
        project: &ProjectId,
        proposal: FollowBatchProposal,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailItemRef>, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(None);
        };
        let mut write = store.begin_admission(self.record_origin()).await?;
        let key = RecordRef {
            kind: RecordKind::SubscriptionMembership,
            id: proposal.membership.clone(),
        };
        let Some(Record::SubscriptionMembership(member)) = write.record(&key).await? else {
            write.rollback().await?;
            return Ok(None);
        };
        let current = write.membership(member.reader(), member.scope()).await?;
        if current.as_ref().is_none_or(|current| {
            current.envelope.id != proposal.membership
                || current.activated_at() != proposal.activated_at
                || current.generation() != proposal.generation
        }) || member.policy().mode() == SubscriptionMode::Off
            || member
                .ended()
                .is_some_and(|end| end.reason != EndReason::Resolved)
            || member.ended().is_none() && now >= member.expires_at()
        {
            write.rollback().await?;
            return Ok(None);
        }
        let invalid = || ProjectAdmissionError::InvalidRecord("follow batch proposal".into());
        let Identity::Session { session } = member.reader() else {
            return Err(invalid());
        };
        if session.endpoint.service_id != proposal.enrollment.service
            || proposal.roots.is_empty()
            || proposal.roots.len() > MAX_SUBSCRIPTION_NOTICE_ROOTS
            || member.policy().mode() == SubscriptionMode::Poll && proposal.roots.len() != 1
        {
            return Err(invalid());
        }
        let excluded = super::follow_coverage_validation::validate_preventions(
            &mut write,
            &member,
            &proposal.preventions,
        )
        .await?;
        if !ranges_are_owed(&mut write, &member, &proposal.roots, &excluded).await? {
            write.rollback().await?;
            return Ok(None);
        }
        let first = proposal.roots.first().ok_or_else(invalid)?;
        let body = BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "New thread activity"
                .to_owned()
                .try_into()
                .map_err(|_| invalid())?,
            pointer: RecordPointer {
                project: project.clone(),
                subject: first.root().clone().into(),
                position: first.through(),
            },
            source: NotificationSource::FollowBatch {
                membership: proposal
                    .membership
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| invalid())?,
                activated_at: member.activated_at(),
                generation: member.generation(),
                roots: proposal.roots,
                policy: member.policy().clone(),
                draining: member.ended().is_some(),
            },
        });
        let item = DeliveryItem::<BoardDeliveryTypes>::new(
            proposal.item,
            MailSender::Project {
                project: project.clone(),
            },
            Recipient {
                peer: proposal.enrollment.peer,
                address: session.clone(),
            },
            None,
            body,
        )
        .map_err(|_| invalid())?;
        let handoff = MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(item)));
        let item = handoff.id()?;
        write.append_mail_handoff(&handoff).await?;
        write.commit().await?;
        self.registry.notify_activity(project);
        Ok(Some(item))
    }
}

async fn ranges_are_owed(
    write: &mut ProjectWriteTransaction,
    member: &SubscriptionMembership,
    roots: &[RootRange],
    excluded: &std::collections::HashSet<ItemId>,
) -> Result<bool, ProjectAdmissionError> {
    let head = write.high_water().await?;
    let upper = member.ended().map_or(head, |end| end.position);
    for (index, root) in roots.iter().enumerate() {
        if root.from().position <= member.activated_at().position
            || root.through().position > upper.position
            || roots
                .iter()
                .take(index)
                .any(|prior| ranges_overlap(prior, root))
            || matches!(member.scope(), SubscriptionScope::Thread { thread_id } if thread_id != root.root())
        {
            return Ok(false);
        }
        if matches!(member.scope(), SubscriptionScope::Project { .. })
            && write
                .membership(
                    member.reader(),
                    &SubscriptionScope::thread(root.root().clone()),
                )
                .await?
                .is_some()
        {
            return Ok(false);
        }
        let Some(watched_after) = write
            .effective_thread_watch(root.root(), member.reader())
            .await?
        else {
            return Ok(false);
        };
        if root.from().position <= watched_after.position {
            return Ok(false);
        }
        if !matches!(
            write.cursor_validity(root.from()).await?,
            message_board_storage::CursorValidity::Valid
        ) || !matches!(
            write.cursor_validity(root.through()).await?,
            message_board_storage::CursorValidity::Valid
        ) {
            return Ok(false);
        }
        let mut after = member.activated_at();
        let mut count = 0_u64;
        let mut first = None;
        let mut last = None;
        while after.position < root.through().position {
            let limit = (root.through().position.get() - after.position.get()).min(100);
            let FeedRead::Entries {
                entries,
                covered_through,
                ..
            } = write.feed_after(after, limit, false).await?
            else {
                return Ok(false);
            };
            for entry in entries {
                if entry.position < root.from().position || entry.time.as_router_time().is_none() {
                    continue;
                }
                let FeedContent::Shown(record) = entry.content else {
                    continue;
                };
                let Record::Post(post) = *record else {
                    continue;
                };
                let thread = match &post.placement {
                    Placement::Thread { thread_id } => thread_id.clone(),
                    Placement::Project { .. } => ThreadId::from(post.post_id.clone()),
                };
                if &thread == root.root() && post.actor != *member.reader() {
                    let cursor = FeedCursor {
                        epoch: entry.epoch,
                        position: entry.position,
                    };
                    first.get_or_insert(cursor);
                    last = Some(cursor);
                    count = count.checked_add(1).ok_or_else(|| {
                        ProjectAdmissionError::InvalidRecord("follow post count".into())
                    })?;
                }
            }
            after = covered_through;
        }
        if first != Some(root.from()) || last != Some(root.through()) || count != root.post_count()
        {
            return Ok(false);
        }
    }
    let mut after = HandoffSeq::ZERO;
    loop {
        let page = write.mail_handoff_page(after, 100).await?;
        if page.is_empty() {
            break;
        }
        for handoff in page {
            after = handoff.sequence;
            let MailHandoffItem::Delivery(envelope) = handoff.item else {
                continue;
            };
            if excluded.contains(&envelope.id()) {
                continue;
            }
            let DeliveryEnvelope::Item(item) = *envelope else {
                continue;
            };
            let BoardDeliveryBody::Notification(body) = item.body() else {
                continue;
            };
            if let NotificationSource::FollowBatch {
                membership,
                activated_at,
                roots: covered,
                ..
            } = &body.source
                && membership.as_str() == member.envelope.id.as_str()
                && *activated_at == member.activated_at()
                && covered
                    .iter()
                    .any(|prior| roots.iter().any(|root| ranges_overlap(prior, root)))
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn ranges_overlap(left: &RootRange, right: &RootRange) -> bool {
    left.root() == right.root()
        && left.from().position <= right.through().position
        && right.from().position <= left.through().position
}
