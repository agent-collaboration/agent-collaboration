//! Membership grouping and replicated coverage bound each local follow proposal.
use super::{
    BoardError, DateTime, EndReason, FeedCursor, HandoffSeq, Identity,
    MAX_SUBSCRIPTION_NOTICE_ROOTS, ProjectId, RecordId, SubscriptionGeneration,
    SubscriptionMembership, SubscriptionMode, SubscriptionWindowStore, ThreadId, Utc,
    WindowStoreError, membership_mode_off_or_expired, project_registry_failure,
    project_storage_failure, unavailable,
};
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{NotificationSource, RootRange};
use fleet_sync_protocol::DeliveryEnvelope;
use message_board_storage::MailHandoffItem;

#[cfg(test)]
pub(in crate::service_identity::subscription_delivery) struct ExpiryReadProbe {
    entered: tokio::sync::Notify,
    released: tokio::sync::Notify,
}
#[cfg(test)]
impl ExpiryReadProbe {
    pub async fn wait_for_read(&self) {
        self.entered.notified().await;
    }
    pub fn release(&self) {
        self.released.notify_one();
    }
}

pub(in crate::service_identity::subscription_delivery) struct FollowWindowProposal {
    pub project: ProjectId,
    pub membership: RecordId,
    pub activated_at: FeedCursor,
    pub generation: SubscriptionGeneration,
    pub roots: Vec<RootRange>,
    pub preventions: Vec<crate::notification_publisher::GenerationPreventedFollow>,
}

impl SubscriptionWindowStore {
    /// Exercise the owning expiry read while a Host's receiving install is held.
    #[cfg(feature = "runtime-test-support")]
    pub async fn scan_membership_expiry_for_test(
        &self,
        reader: &Identity,
        projects: &[ProjectId],
    ) -> Result<(), BoardError> {
        self.membership_expiry_candidates(reader, projects)
            .await
            .map(|_| ())
    }

    #[cfg(test)]
    pub(in crate::service_identity::subscription_delivery) async fn pause_next_expiry_read_for_test(
        &self,
    ) -> Arc<ExpiryReadProbe> {
        let probe = Arc::new(ExpiryReadProbe {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        *self.expiry_read_probe.lock().await = Some(probe.clone());
        probe
    }
    pub(in crate::service_identity::subscription_delivery) async fn membership_expiry_candidates(
        &self,
        reader: &Identity,
        projects: &[ProjectId],
    ) -> Result<
        Vec<(
            ProjectId,
            crate::project_admission::MembershipExpiryProposal,
        )>,
        BoardError,
    > {
        let mut proposals = Vec::new();
        for member in self.memberships_in_projects(Some(reader), projects).await? {
            if member.ended().is_none() {
                let project = member
                    .envelope
                    .scope
                    .project
                    .clone()
                    .ok_or_else(BoardError::board_unavailable)?;
                proposals.push((
                    project,
                    crate::project_admission::MembershipExpiryProposal {
                        membership: member.envelope.id.clone(),
                        activated_at: member.activated_at(),
                        generation: member.generation(),
                        expires_at: member.expires_at(),
                    },
                ));
            }
        }
        #[cfg(test)]
        if let Some(probe) = self.expiry_read_probe.lock().await.take() {
            probe.entered.notify_one();
            probe.released.notified().await;
        }
        Ok(proposals)
    }

    pub(in crate::service_identity::subscription_delivery) async fn propose_follow_batches(
        &self,
        reader: &Identity,
        due: &[ThreadId],
        now: DateTime<Utc>,
        projects: &[ProjectId],
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<Vec<FollowWindowProposal>, BoardError> {
        let mut proposals = Vec::new();
        for membership in self.memberships_in_projects(Some(reader), projects).await? {
            let loaded = self.load_with_preventions(membership, preventions).await?;
            if loaded.membership.policy().mode() == SubscriptionMode::Off
                || membership_mode_off_or_expired(&loaded.membership, now)
                || loaded
                    .membership
                    .ended()
                    .is_some_and(|end| end.reason != EndReason::Resolved)
            {
                continue;
            }
            let roots = loaded
                .progress
                .roots
                .iter()
                .filter(|root| due.contains(&root.thread_id))
                .filter_map(|root| root.window.as_ref().map(|window| (root, window)))
                .take(MAX_SUBSCRIPTION_NOTICE_ROOTS)
                .map(|(root, window)| {
                    RootRange::new(
                        root.thread_id.clone(),
                        window.from,
                        window.through,
                        window.post_count,
                    )
                    .map_err(unavailable)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let roots_per_item = match loaded.membership.policy().mode() {
                SubscriptionMode::Poll => 1,
                SubscriptionMode::Deliver => MAX_SUBSCRIPTION_NOTICE_ROOTS,
                SubscriptionMode::Off => continue,
            };
            for roots in roots.chunks(roots_per_item) {
                proposals.push(FollowWindowProposal {
                    preventions: preventions
                        .iter()
                        .filter(|fact| fact.belongs_to(&loaded.membership))
                        .cloned()
                        .collect(),
                    project: loaded.progress.project.clone(),
                    membership: loaded.membership.envelope.id.clone(),
                    activated_at: loaded.membership.activated_at(),
                    generation: loaded.membership.generation(),
                    roots: roots.to_vec(),
                });
            }
        }
        Ok(proposals)
    }

    pub(super) async fn follow_coverage(
        &self,
        membership: &SubscriptionMembership,
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<Vec<RootRange>, WindowStoreError> {
        let project = membership
            .envelope
            .scope
            .project
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        let store = self
            .registry
            .existing_project(project)
            .await
            .map_err(|error| project_registry_failure(project, error))?
            .ok_or_else(|| WindowStoreError::RetryProject {
                project: project.clone(),
                error: BoardError::board_unavailable(),
                diagnostic: "project disappeared during follow coverage".into(),
            })?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        let frontier = read
            .mail_settled_through()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        let mut after = HandoffSeq::ZERO;
        let mut coverage = Vec::new();
        loop {
            let page = read
                .mail_handoff_page(after, 100)
                .await
                .map_err(|error| project_storage_failure(project, error))?;
            if page.is_empty() {
                break;
            }
            for handoff in page {
                after = handoff.sequence;
                let MailHandoffItem::Delivery(envelope) = handoff.item else {
                    continue;
                };
                if handoff.sequence > frontier
                    && preventions
                        .iter()
                        .any(|fact| fact.excludes(project, &envelope))
                {
                    continue;
                }
                let DeliveryEnvelope::Item(item) = *envelope else {
                    continue;
                };
                let BoardDeliveryBody::Notification(body) = item.body() else {
                    continue;
                };
                if let NotificationSource::FollowBatch {
                    membership: id,
                    activated_at,
                    roots,
                    ..
                } = &body.source
                    && id.as_str() == membership.envelope.id.as_str()
                    && *activated_at == membership.activated_at()
                {
                    coverage.extend(roots.iter().cloned());
                }
            }
        }
        read.finish()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        Ok(coverage)
    }
}

#[cfg(test)]
use std::sync::Arc;
