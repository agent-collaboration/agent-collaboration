//! An SDK observation cannot bypass the locked immutable board association or live membership.
use super::{MailItemRef, ProjectAdmissionError, ProjectWriteTransaction, SubscriptionMembership};
use crate::notification_publisher::GenerationPreventedFollow;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;
use fleet_sync_protocol::{DeliveryEnvelope, ItemId};
use message_board_storage::{MailCancelGroupCause, MailHandoffItem};
use std::collections::HashSet;

pub(super) async fn validate_preventions(
    write: &mut ProjectWriteTransaction,
    member: &SubscriptionMembership,
    facts: &[GenerationPreventedFollow],
) -> Result<HashSet<ItemId>, ProjectAdmissionError> {
    let invalid = || ProjectAdmissionError::InvalidRecord("generation prevention reference".into());
    let frontier = write.mail_settled_through().await?;
    let mut excluded = HashSet::new();
    for fact in facts {
        if !fact.belongs_to(member) {
            return Err(invalid());
        }
        let group = write
            .mail_handoff(fact.group())
            .await?
            .ok_or_else(invalid)?;
        let MailHandoffItem::CancelGroup(group_item) = group.item else {
            return Err(invalid());
        };
        if group_item.cause != MailCancelGroupCause::GenerationChanged
            || group_item.membership.as_str() != member.envelope.id.as_str()
            || group_item.activated_at != member.activated_at()
            || group_item.generation.get() >= member.generation().get()
        {
            return Err(invalid());
        }
        let target_id: MailItemRef = fact
            .target()
            .item
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|_| invalid())?;
        let target = write.mail_handoff(&target_id).await?.ok_or_else(invalid)?;
        if target.sequence <= frontier || target.sequence >= group.sequence {
            return Err(invalid());
        }
        let MailHandoffItem::Delivery(envelope) = target.item else {
            return Err(invalid());
        };
        if envelope.id() != fact.target().item || envelope.digest() != &fact.target().digest {
            return Err(invalid());
        }
        let DeliveryEnvelope::Item(item) = *envelope else {
            return Err(invalid());
        };
        let BoardDeliveryBody::Notification(body) = item.body() else {
            return Err(invalid());
        };
        if !matches!(&body.source, NotificationSource::FollowBatch { membership, activated_at, generation, .. }
            if membership == &group_item.membership && activated_at == &group_item.activated_at && generation == &group_item.generation)
        {
            return Err(invalid());
        }
        let child = write
            .mail_cancel_expansion_for_target(fact.group(), &target_id)
            .await?
            .ok_or_else(invalid)?;
        if child.expansion.envelope.id() != fact.cancel().item
            || child.expansion.envelope.digest() != &fact.cancel().digest
        {
            return Err(invalid());
        }
        excluded.insert(fact.target().item);
    }
    Ok(excluded)
}
