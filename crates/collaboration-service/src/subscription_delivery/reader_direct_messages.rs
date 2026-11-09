//! DM presence, submission and response handling inside the target's single delivery actor.
use super::{
    BoardError, LoadPolicy, PushRecord, ReaderDeliveryOwner, TargetPresence, target_session,
};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_protocol::{MessageDelivery, PushDeliveryState};

impl ReaderDeliveryOwner {
    pub(super) async fn deliver_direct_messages(&mut self) -> Result<bool, BoardError> {
        let Ok(target) = target_session(&self.reader) else {
            return Ok(false);
        };
        let native_pending = self.resume_native_held_mail(&target).await?;
        let records = self.push.direct_messages(&target).await?;
        self.dm_checked
            .retain(|push_id| records.iter().any(|record| &record.push_id == push_id));
        for record in &records {
            // A held DM is checked once, then again only after a signal that its target
            // may have loaded. It is marked checked only once its write succeeds, so a
            // failed pass checks and writes it again on the backoff.
            if self.dm_checked.contains(&record.push_id) {
                continue;
            }
            let recorded = self.held_mail.record_position(self.holds_mail()).await;
            // Wakeable establishes unloaded presence. Unreachable is ambiguous:
            // Layer 0 must still classify live-elsewhere and permanent route failures.
            let known_unloaded = matches!(
                self.presence.presence(&target).await,
                Ok(TargetPresence::Wakeable)
            );
            let new_auto = collaboration_runtime::mail_push_preparation::direct_message_load_policy(
                record.mode,
                record.guard.as_ref(),
                record.delivery_state,
            ) == LoadPolicy::MayLoad;
            let settled = if !known_unloaded || new_auto {
                self.push.deliver_direct_message(record).await?
            } else if record.mode == Some(MessageDelivery::Steer) || record.guard.is_some() {
                self.push.reject_direct_message(record).await?
            } else {
                self.push.hold_direct_message(record).await?
            };
            #[cfg(test)]
            self.observe(if settled.delivery_state == PushDeliveryState::Held {
                OwnerObservation::DirectMessageHeld
            } else {
                OwnerObservation::DirectMessageSettled
            });
            self.answer_direct_message(&settled);
            if settled.delivery_state == PushDeliveryState::Held {
                // A DM stays held only where a journal position says when to check again.
                recorded?;
                self.dm_checked.insert(record.push_id.clone());
            }
        }
        // An accepted command may name work settled on an earlier scan. Read back its
        // durable result rather than replaying the effect to satisfy the response.
        let unanswered = self.dm_replies.keys().cloned().collect::<Vec<_>>();
        for push_id in unanswered {
            let record = self.push.direct_message(&push_id).await?;
            if record.last_outcome.is_some() {
                self.answer_direct_message(&record);
            }
        }
        Ok(native_pending || !self.push.direct_messages(&target).await?.is_empty())
    }

    fn answer_direct_message(&mut self, record: &PushRecord) {
        if let Some(replies) = self.dm_replies.remove(&record.push_id) {
            for reply in replies {
                let _ = reply.send(Ok(record.clone()));
            }
        }
    }
}
