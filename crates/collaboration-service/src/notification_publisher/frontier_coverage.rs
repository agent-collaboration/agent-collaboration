//! A prevented generation is settled only when its still-owed activity has terminal coverage.
use super::settled_frontier::{FollowCompletion, MailFrontierContext};
use super::{BoardError, ProjectId, PublisherInner};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::NotificationSource;
use fleet_sync_protocol::DeliveryEnvelope;
use message_board::{
    EndReason, FeedContent, FeedRead, HandoffSeq, Placement, Record, RecordKind, RecordRef,
    SubscriptionMode, ThreadId,
};
use message_board_storage::MailHandoffItem;

impl PublisherInner {
    pub(super) async fn replacement_ranges_settled(
        &self,
        project: &ProjectId,
        context: &MailFrontierContext,
        original: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<bool, BoardError> {
        let DeliveryEnvelope::Item(item) = original else {
            return Err(BoardError::board_unavailable());
        };
        let BoardDeliveryBody::Notification(body) = item.body() else {
            return Err(BoardError::board_unavailable());
        };
        let NotificationSource::FollowBatch {
            membership,
            activated_at,
            roots,
            ..
        } = &body.source
        else {
            return Err(BoardError::board_unavailable());
        };
        let store = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let key = RecordRef {
            kind: RecordKind::SubscriptionMembership,
            id: membership
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| BoardError::board_unavailable())?,
        };
        let member = read
            .membership_activation(&key.id, *activated_at)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let Some(member) = member else {
            return Err(BoardError::board_unavailable());
        };
        if member.activated_at() != *activated_at {
            return Err(BoardError::board_unavailable());
        }
        if member
            .ended()
            .is_some_and(|end| end.reason != EndReason::Resolved)
            || member.policy().mode() == SubscriptionMode::Off
        {
            return Ok(true);
        }

        let mut coverage = Vec::new();
        let mut after = HandoffSeq::ZERO;
        loop {
            let page = self.frontier_handoff_page(project, context, after).await?;
            if page.is_empty() {
                break;
            }
            for candidate in page {
                after = candidate.sequence;
                let MailHandoffItem::Delivery(envelope) = candidate.item else {
                    continue;
                };
                if envelope.id() == original.id() {
                    continue;
                }
                let DeliveryEnvelope::Item(item) = &*envelope else {
                    continue;
                };
                let BoardDeliveryBody::Notification(body) = item.body() else {
                    continue;
                };
                let NotificationSource::FollowBatch {
                    membership: id,
                    activated_at: activation,
                    roots: ranges,
                    ..
                } = &body.source
                else {
                    continue;
                };
                if id != membership || activation != activated_at {
                    continue;
                }
                if self
                    .follow_completion(project, context, candidate.sequence, &envelope)
                    .await?
                    == FollowCompletion::Covering
                {
                    coverage.extend(ranges.iter().cloned());
                }
            }
        }
        // Like the allocator, examine actual followed posts, not numeric gaps
        // occupied by another root, the reader's own posts, or imported history.
        // All SDK/network observations above finished before this snapshot opens.
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let mut covered = true;
        'roots: for root in roots {
            let mut after = *activated_at;
            while after.position < root.through().position {
                let limit = (root.through().position.get() - after.position.get()).min(100);
                let FeedRead::Entries {
                    entries,
                    covered_through,
                    ..
                } = read
                    .feed_after(after, limit, false)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
                else {
                    covered = false;
                    break 'roots;
                };
                if covered_through.position <= after.position {
                    return Err(BoardError::board_unavailable());
                }
                for entry in entries {
                    if entry.position < root.from().position
                        || entry.time.as_router_time().is_none()
                    {
                        continue;
                    }
                    let FeedContent::Shown(record) = entry.content else {
                        continue;
                    };
                    let Record::Post(post) = *record else {
                        continue;
                    };
                    if post.actor == *member.reader() {
                        continue;
                    }
                    let thread = match post.placement {
                        Placement::Thread { thread_id } => thread_id,
                        Placement::Project { .. } => ThreadId::from(post.post_id),
                    };
                    if &thread == root.root()
                        && !coverage.iter().any(|range| {
                            range.root() == root.root()
                                && range.from().position <= entry.position
                                && range.through().position >= entry.position
                        })
                    {
                        covered = false;
                        break 'roots;
                    }
                }
                after = covered_through;
            }
        }
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        Ok(covered)
    }
}
