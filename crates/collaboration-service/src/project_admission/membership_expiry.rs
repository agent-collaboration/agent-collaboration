//! A stale expiry observation cannot end a renewed membership under the project lock.
use super::{ProjectAdmission, ProjectAdmissionError};
use chrono::{DateTime, Utc};
use message_board::{
    EndReason, FeedCursor, MembershipEnd, ProjectId, Record, RecordId, RecordKind, RecordRef,
    RecordTime, RecordedActor, RouterComponent, SubscriptionGeneration, SubscriptionMembership,
    SubscriptionMembershipProps,
};

#[derive(Clone)]
pub(crate) struct MembershipExpiryProposal {
    pub membership: RecordId,
    pub activated_at: FeedCursor,
    pub generation: SubscriptionGeneration,
    pub expires_at: DateTime<Utc>,
}

impl ProjectAdmission {
    pub(crate) async fn expire_follow_membership(
        &self,
        project: &ProjectId,
        proposal: MembershipExpiryProposal,
        now: DateTime<Utc>,
    ) -> Result<bool, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(false);
        };
        let mut write = store.begin_admission(self.record_origin()).await?;
        let key = RecordRef {
            kind: RecordKind::SubscriptionMembership,
            id: proposal.membership,
        };
        let Some(Record::SubscriptionMembership(member)) = write.record(&key).await? else {
            write.rollback().await?;
            return Ok(false);
        };
        if member.ended().is_some()
            || member.activated_at() != proposal.activated_at
            || member.generation() != proposal.generation
            || member.expires_at() != proposal.expires_at
            || now < member.expires_at()
        {
            write.rollback().await?;
            return Ok(false);
        }
        let high_water = write.high_water().await?;
        let cursor = FeedCursor {
            epoch: write.admitted_epoch()?,
            position: high_water.position.successor()?,
        };
        let mut props: SubscriptionMembershipProps = member.into();
        props.envelope.actor = RecordedActor::Router {
            component: RouterComponent::SubscriptionDelivery,
            installation: self.installation.clone(),
        };
        props.envelope.request = None;
        props.envelope.position = write.record_placement(cursor);
        props.envelope.time = RecordTime::Router(now);
        props.ended = Some(MembershipEnd {
            reason: EndReason::Expired,
            position: cursor,
            at: now,
        });
        let ended = SubscriptionMembership::new(props)
            .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        super::membership_mail_cancels::record_membership_cancel(&mut write, &ended).await?;
        write
            .append_record(Record::SubscriptionMembership(ended))
            .await?;
        write.commit().await?;
        self.registry.notify_activity(project);
        Ok(true)
    }
}
