//! Inline cleanup of copied held snapshots after every covering root has ended.
use super::{
    BoardError, Identity, PushRecord, ReaderDeliveryOwner, SubscriptionScope, target_session,
};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_protocol::PushKind;

impl ReaderDeliveryOwner {
    /// One attempt per trigger/pass. Failure only retains a small marker and a backoff
    /// retry time; it never parks the Reader in a retry loop or carries a stale selection.
    pub(in super::super) async fn try_pending_held_push_cleanup(&mut self) {
        let Some(reason) = self.pending_ended_push_cleanup.clone() else {
            return;
        };
        if matches!(self.reader, Identity::Human { .. }) {
            self.finish_held_push_cleanup();
            return;
        }
        if let Err(error) = target_session(&self.reader) {
            tracing::warn!(%error, field = "reader", "held-push cleanup has no target session; dropping marker");
            self.finish_held_push_cleanup();
            return;
        }
        match self.settle_selected_held_pushes(&reason).await {
            Ok(_settled) => {
                #[cfg(test)]
                self.observe(OwnerObservation::HeldSubscriptionPushesSettled(_settled));
                if self.held_cleanup_backoff.record_success() {
                    tracing::info!(
                        held_cleanup_retry = "recovered",
                        "held-push cleanup recovered"
                    );
                }
                self.finish_held_push_cleanup();
            }
            Err(error) => {
                let retry = self.held_cleanup_backoff.record_failure();
                if retry.starts_run {
                    tracing::warn!(%error, %reason, field = "heldSubscriptionPushes", held_cleanup_retry = "failing", "held-push cleanup failed; retrying with backoff");
                }
                let now = self.clock.monotonic_now();
                self.held_cleanup_retry_at = Some(now.checked_add(retry.delay).unwrap_or(now));
            }
        }
    }

    fn finish_held_push_cleanup(&mut self) {
        self.pending_ended_push_cleanup = None;
        self.held_cleanup_retry_at = None;
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
                    self.clock.utc_now(),
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
                .get_thread_subscription_record(&self.reader, &thread_scope, self.clock.utc_now())
                .await?
            {
                Some(record) => Some(record), // Thread overrides also shadow Topic scopes when ended.
                None => {
                    board
                        .get_thread_subscription_record(
                            &self.reader,
                            &SubscriptionScope::project(range.project_id.clone()),
                            self.clock.utc_now(),
                        )
                        .await?
                }
            };
            if covering.is_some_and(|record| {
                record.state().is_delivery_eligible() && record.expires_at() > self.clock.utc_now()
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
