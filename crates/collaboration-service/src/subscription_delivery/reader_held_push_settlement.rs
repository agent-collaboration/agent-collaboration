//! Inline cleanup of copied held snapshots after every covering root has ended.
use super::{
    BoardError, Identity, PushRecord, ReaderDeliveryOwner, SubscriptionScope, target_session,
};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_protocol::PushKind;

impl ReaderDeliveryOwner {
    /// One attempt per trigger/pass. Failure only retains a small marker; it
    /// never parks the Reader in a retry loop or carries a stale selection.
    pub(in super::super) async fn try_pending_held_push_cleanup(&mut self) {
        let Some(reason) = self.pending_ended_push_cleanup.clone() else {
            return;
        };
        if matches!(self.reader, Identity::Human { .. }) {
            self.pending_ended_push_cleanup = None;
            return;
        }
        if let Err(error) = target_session(&self.reader) {
            tracing::warn!(%error, field = "reader", "held-push cleanup has no target session; dropping marker");
            self.pending_ended_push_cleanup = None;
            return;
        }
        if self.try_held_push_cleanup(&reason).await {
            self.pending_ended_push_cleanup = None;
        }
    }

    async fn try_held_push_cleanup(&self, reason: &str) -> bool {
        match self.settle_selected_held_pushes(reason).await {
            Ok(_settled) => {
                #[cfg(test)]
                self.observe(OwnerObservation::HeldSubscriptionPushesSettled(_settled));
                true
            }
            Err(error) => {
                tracing::warn!(%error, %reason, field = "heldSubscriptionPushes", "held-push cleanup deferred to next owner pass");
                false
            }
        }
    }

    async fn settle_selected_held_pushes(&self, reason: &str) -> Result<u64, BoardError> {
        let target = target_session(&self.reader)?;
        let candidates = self
            .push
            .store
            .list_held_carrier_push_records(&target)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let mut settled = 0;
        for record in candidates
            .into_iter()
            .filter(|record| record.kind == PushKind::SubscriptionActivity)
        {
            // Ended cleanup is about the entire snapshot, not just one scope
            // that happened to disappear from the owner's current record list.
            if !self.all_push_roots_ended(&record).await? {
                continue;
            }
            settled += self
                .push
                .store
                .settle_held_subscription_push(
                    &target,
                    &record.push_id,
                    None,
                    reason,
                    self.clock.now(),
                )
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        }
        Ok(settled)
    }

    async fn all_push_roots_ended(&self, record: &PushRecord) -> Result<bool, BoardError> {
        let Some(activity) = record.activity.as_ref() else {
            return Ok(false);
        };
        if activity.ranges.is_empty() {
            return Ok(false);
        }
        let board = self.board_store()?.lock().await;
        for range in &activity.ranges {
            let thread_scope = SubscriptionScope::thread(range.thread_id.clone());
            let covering = match board
                .get_thread_subscription_record(&self.reader, &thread_scope, self.clock.now())
                .await?
            {
                Some(record) => Some(record), // Thread overrides also shadow Topic scopes when ended.
                None => {
                    board
                        .get_thread_subscription_record(
                            &self.reader,
                            &SubscriptionScope::project(range.project_id.clone()),
                            self.clock.now(),
                        )
                        .await?
                }
            };
            if covering.is_some_and(|record| {
                record.state().is_delivery_eligible() && record.expires_at() > self.clock.now()
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
