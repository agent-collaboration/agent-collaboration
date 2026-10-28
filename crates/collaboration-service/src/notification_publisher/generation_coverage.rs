//! Per-pass SDK evidence names immutable references; admission revalidates their board cause.
use super::{BoardError, NotificationPublisher, ProjectId, publication_epoch_if_writable};
use crate::mail_delivery_runtime::{FrozenCancelSettlement, RemoteCancelReplay};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItemRef};
use message_board::{CancelExpansionSeq, MailItemRef};
use message_board_storage::{MailCancelGroupCause, MailHandoffItem};

/// Only the publisher constructs this non-serializable, transient observation.
#[derive(Clone)]
pub(crate) struct GenerationPreventedFollow {
    project: ProjectId,
    group: MailItemRef,
    target: DeliveryItemRef,
    cancel: DeliveryItemRef,
    membership: collaboration_mail_protocol::MembershipId,
    activated_at: message_board::FeedCursor,
}
impl GenerationPreventedFollow {
    #[cfg(test)]
    pub(crate) fn substituted_for_test(
        &self,
        group: MailItemRef,
        target: DeliveryItemRef,
        cancel: DeliveryItemRef,
    ) -> Self {
        Self {
            group,
            target,
            cancel,
            ..self.clone()
        }
    }
    pub(crate) fn group(&self) -> &MailItemRef {
        &self.group
    }
    pub(crate) fn target(&self) -> &DeliveryItemRef {
        &self.target
    }
    pub(crate) fn cancel(&self) -> &DeliveryItemRef {
        &self.cancel
    }
    pub(crate) fn belongs_to(&self, member: &message_board::SubscriptionMembership) -> bool {
        member.envelope.scope.project.as_ref() == Some(&self.project)
            && self.membership.as_str() == member.envelope.id.as_str()
            && self.activated_at == member.activated_at()
    }
    pub(crate) fn excludes(
        &self,
        project: &ProjectId,
        envelope: &DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>,
    ) -> bool {
        &self.project == project
            && self.target.item == envelope.id()
            && &self.target.digest == envelope.digest()
    }
}

impl NotificationPublisher {
    pub(crate) async fn generation_preventions(
        &self,
        projects: &[ProjectId],
        memberships: &[message_board::SubscriptionMembership],
    ) -> Result<Vec<GenerationPreventedFollow>, BoardError> {
        let runtime = self
            .inner
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        let mut facts = Vec::new();
        for project in projects {
            let Some(store) = self
                .inner
                .admission
                .registry
                .existing_project(project)
                .await
                .map_err(|_| BoardError::board_unavailable())?
            else {
                continue;
            };
            let mut after = CancelExpansionSeq::ZERO;
            loop {
                let mut read = store
                    .snapshot()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                let frontier = read
                    .mail_settled_through()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;

                let page = read
                    .mail_cancel_expansion_page(after, 100)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                let mut candidates = Vec::new();
                for child in page {
                    after = child.sequence;
                    let expansion = child.expansion;
                    let group = read
                        .mail_handoff(&expansion.group_handoff)
                        .await
                        .map_err(|_| BoardError::board_unavailable())?
                        .ok_or_else(BoardError::board_unavailable)?;
                    let group_sequence = group.sequence;
                    let MailHandoffItem::CancelGroup(group) = group.item else {
                        continue;
                    };
                    // A reader's refresh must never wait on another membership's
                    // SDK lookup or canonical remote Query in the same project.
                    if group.cause != MailCancelGroupCause::GenerationChanged
                        || !memberships.iter().any(|member| {
                            member.envelope.scope.project.as_ref() == Some(project)
                                && member.envelope.id.as_str() == group.membership.as_str()
                                && member.activated_at() == group.activated_at
                        })
                    {
                        continue;
                    }
                    let target = read
                        .mail_handoff(&expansion.target_item)
                        .await
                        .map_err(|_| BoardError::board_unavailable())?
                        .ok_or_else(BoardError::board_unavailable)?;
                    if target.sequence <= frontier {
                        continue;
                    }
                    if let MailHandoffItem::Delivery(envelope) = target.item {
                        candidates.push((
                            group_sequence,
                            child.sequence,
                            group,
                            envelope,
                            expansion.envelope,
                        ));
                    }
                }
                read.finish()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                // Neither a project snapshot nor a reader window gate spans SDK/network I/O.
                for (group_sequence, child_sequence, group, target, cancel) in candidates {
                    let mut replay = RemoteCancelReplay::QueryOnly;
                    if target.recipient().address.endpoint.service_id
                        != self.inner.admission.installation
                    {
                        let marks = runtime.acknowledged_mail_changes(project).await?;
                        if marks
                            .max_handoff_seq
                            .is_some_and(|mark| mark >= group_sequence.get())
                            && marks
                                .max_expansion_seq
                                .is_some_and(|mark| mark >= child_sequence.get())
                            && let Some(epoch) = publication_epoch_if_writable(&store).await?
                        {
                            replay = RemoteCancelReplay::Eligible { epoch };
                        }
                    }
                    if runtime
                        .replay_frozen_cancel_and_observe(&target, &cancel, replay)
                        .await
                        .map_err(|_| BoardError::board_unavailable())?
                        == FrozenCancelSettlement::Prevented
                    {
                        facts.push(GenerationPreventedFollow {
                            project: project.clone(),
                            group: group.id,
                            membership: group.membership,
                            activated_at: group.activated_at,
                            target: DeliveryItemRef {
                                item: target.id(),
                                digest: target.digest().clone(),
                            },
                            cancel: DeliveryItemRef {
                                item: cancel.id(),
                                digest: cancel.digest().clone(),
                            },
                        });
                    }
                }
                // A fresh page read keeps memory bounded while immutable references remain stable.
                let mut read = store
                    .snapshot()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                let empty = read
                    .mail_cancel_expansion_page(after, 1)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?
                    .is_empty();
                read.finish()
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                if empty {
                    break;
                }
            }
        }
        Ok(facts)
    }
}
