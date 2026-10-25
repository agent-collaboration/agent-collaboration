//! Page terminal evidence outside locks, then fence its project context at admission.
use super::*;
use crate::mail_delivery_runtime::{FrozenCancelSettlement, RemoteCancelReplay};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::NotificationSource;
use fleet_sync_protocol::DeliveryEnvelope;
use message_board::{FeedCursor, HandoffSeq, MailItemRef};
use message_board_storage::{
    MailCancelGroup, MailCancelGroupCause, MailHandoff, MailHandoffItem, ProjectStorageError,
    ProjectWriteTransaction,
};

#[derive(Clone, Copy)]
pub(super) struct MailFrontierContext {
    pub epoch: message_board::EpochNumber,
    pub feed: FeedCursor,
    pub highest: HandoffSeq,
    pub settled: HandoffSeq,
}

/// Transient publisher evidence: it cannot arrive over a wire or be stored as a fact.
pub(crate) struct SettledMailPrefix {
    project: ProjectId,
    context: MailFrontierContext,
    through: HandoffSeq,
}
impl SettledMailPrefix {
    pub(crate) fn project(&self) -> &ProjectId {
        &self.project
    }
    pub(crate) fn through(&self) -> HandoffSeq {
        self.through
    }
    pub(crate) async fn still_current(
        &self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<bool, ProjectStorageError> {
        Ok(write.high_water().await? == self.context.feed
            && write.admitted_epoch()? == self.context.epoch
            && write.mail_handoff_head().await? == self.context.highest
            && write.mail_settled_through().await? == self.context.settled
            && self.through > self.context.settled
            && self.through <= self.context.highest)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FollowCompletion {
    Pending,
    Covering,
    GenerationPrevented,
}

impl PublisherInner {
    pub(super) async fn advance_project_frontier(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        let store = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        loop {
            if self.shutdown.is_cancelled() {
                return Ok(());
            }
            let epoch = publication_epoch(&store).await?;
            let mut read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let context = MailFrontierContext {
                epoch,
                feed: read.high_water(),
                highest: read
                    .mail_handoff_head()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?,
                settled: read
                    .mail_settled_through()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?,
            };
            let page = read
                .mail_handoff_page(context.settled, 100)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let mut through = context.settled;
            for handoff in &page {
                if !self
                    .handoff_consequences_settled(project, &context, handoff)
                    .await?
                {
                    break;
                }
                through = handoff.sequence;
            }
            if through == context.settled {
                return Ok(());
            }
            let full_page =
                page.last().is_some_and(|last| last.sequence == through) && page.len() == 100;
            if !self
                .admission
                .advance_mail_frontier(SettledMailPrefix {
                    project: project.clone(),
                    context,
                    through,
                })
                .await
                .map_err(|_| BoardError::board_unavailable())?
                || !full_page
            {
                return Ok(());
            }
        }
    }

    pub(super) async fn frontier_handoff_page(
        &self,
        project: &ProjectId,
        context: &MailFrontierContext,
        after: HandoffSeq,
    ) -> Result<Vec<MailHandoff>, BoardError> {
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
        let mut page = read
            .mail_handoff_page(after, 100)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        page.retain(|row| row.sequence <= context.highest);
        Ok(page)
    }

    async fn handoff_consequences_settled(
        &self,
        project: &ProjectId,
        context: &MailFrontierContext,
        handoff: &MailHandoff,
    ) -> Result<bool, BoardError> {
        match &handoff.item {
            MailHandoffItem::Delivery(envelope) => match self
                .follow_completion(project, context, handoff.sequence, envelope)
                .await?
            {
                FollowCompletion::Pending => Ok(false),
                FollowCompletion::Covering => Ok(true),
                FollowCompletion::GenerationPrevented => {
                    self.replacement_ranges_settled(project, context, envelope)
                        .await
                }
            },
            MailHandoffItem::CancelGroup(group) => {
                let mut after = context.settled;
                loop {
                    let page = self.frontier_handoff_page(project, context, after).await?;
                    if page.is_empty() {
                        return Ok(true);
                    }
                    for target in page {
                        if target.sequence >= handoff.sequence {
                            return Ok(true);
                        }
                        after = target.sequence;
                        let MailHandoffItem::Delivery(envelope) = target.item else {
                            continue;
                        };
                        if !follow_matches(group, &envelope) {
                            continue;
                        }
                        match self
                            .observe_group_child(
                                project,
                                context,
                                handoff.sequence,
                                group,
                                &envelope,
                            )
                            .await?
                        {
                            FrozenCancelSettlement::Pending => return Ok(false),
                            FrozenCancelSettlement::Prevented
                                if group.cause == MailCancelGroupCause::GenerationChanged
                                    && !self
                                        .replacement_ranges_settled(project, context, &envelope)
                                        .await? =>
                            {
                                return Ok(false);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    pub(super) async fn follow_completion(
        &self,
        project: &ProjectId,
        context: &MailFrontierContext,
        sequence: HandoffSeq,
        envelope: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<FollowCompletion, BoardError> {
        if sequence <= context.settled {
            return Ok(FollowCompletion::Covering);
        }
        let mail = self
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        if !mail
            .frozen_delivery_is_terminal(envelope)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        {
            return Ok(FollowCompletion::Pending);
        }
        let mut after = sequence;
        let mut prevented = false;
        loop {
            let page = self.frontier_handoff_page(project, context, after).await?;
            if page.is_empty() {
                break;
            }
            for row in page {
                after = row.sequence;
                let MailHandoffItem::CancelGroup(group) = row.item else {
                    continue;
                };
                if group.cause != MailCancelGroupCause::GenerationChanged
                    || !follow_matches(&group, envelope)
                {
                    continue;
                }
                match self
                    .observe_group_child(project, context, row.sequence, &group, envelope)
                    .await?
                {
                    FrozenCancelSettlement::Pending => return Ok(FollowCompletion::Pending),
                    FrozenCancelSettlement::Prevented => prevented = true,
                    FrozenCancelSettlement::SettledWithoutPrevention => {}
                }
            }
        }
        Ok(if prevented {
            FollowCompletion::GenerationPrevented
        } else {
            FollowCompletion::Covering
        })
    }

    async fn observe_group_child(
        &self,
        project: &ProjectId,
        context: &MailFrontierContext,
        group_sequence: HandoffSeq,
        group: &MailCancelGroup,
        target: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<FrozenCancelSettlement, BoardError> {
        let target_id: MailItemRef = target
            .id()
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|_| BoardError::board_unavailable())?;
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
        let child = read
            .mail_cancel_expansion_for_target(&group.id, &target_id)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let Some(child) = child else {
            return Ok(FrozenCancelSettlement::Pending);
        };
        let mail = self
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        let replay =
            if target.recipient().address.endpoint.service_id == self.admission.installation {
                RemoteCancelReplay::QueryOnly
            } else {
                let marks = mail.acknowledged_mail_changes(project).await?;
                if !marks
                    .max_handoff_seq
                    .is_some_and(|mark| mark >= group_sequence.get())
                    || !marks
                        .max_expansion_seq
                        .is_some_and(|mark| mark >= child.sequence.get())
                {
                    return Ok(FrozenCancelSettlement::Pending);
                }
                RemoteCancelReplay::Eligible {
                    epoch: context.epoch,
                }
            };
        mail.replay_frozen_cancel_and_observe(target, &child.expansion.envelope, replay)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
}

fn follow_matches(
    group: &MailCancelGroup,
    envelope: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> bool {
    matches!(envelope, DeliveryEnvelope::Item(item)
        if matches!(item.body(), BoardDeliveryBody::Notification(body)
            if matches!(&body.source, NotificationSource::FollowBatch { .. })
                && super::local_cancellation::group_covers(group, &body.source)))
}
