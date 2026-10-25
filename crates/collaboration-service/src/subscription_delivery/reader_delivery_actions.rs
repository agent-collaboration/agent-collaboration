//! Subscription lifecycle, policy decisions, and durable board settlement.
use super::super::subscription_windows::WindowWrite;
use super::*;
#[path = "reader_held_push_settlement.rs"]
mod reader_held_push_settlement;

impl ReaderDeliveryOwner {
    pub(super) async fn expiry_notices(&mut self, records: &[ThreadSubscriptionRecord]) {
        let Ok(store) = self.board_store() else {
            return;
        };
        let mut ended_reason = None;
        for prior in &self.prior_records.records {
            let project = &prior.project;
            let prior = &prior.record;
            if records.iter().any(|record| record.scope() == prior.scope()) {
                continue;
            }
            let expired = store
                .lock()
                .await
                .subscription_record_in_project(
                    &self.reader,
                    prior.scope(),
                    project,
                    self.clock.now(),
                )
                .await;
            let Ok(Some(expired)) = expired else {
                continue;
            };
            let Some(end_reason) = expired.state().end_reason() else {
                continue;
            };
            ended_reason.get_or_insert(end_reason);
            if end_reason != EndReason::Expired
                || expired.policy().mode() != SubscriptionMode::Deliver
            {
                continue;
            }
            let Ok(target) = target_session(&self.reader) else {
                continue;
            };
            if !matches!(
                self.presence.presence(&target).await,
                Ok(TargetPresence::Running)
            ) {
                continue;
            }
            if let Ok(prepared) = self
                .push
                .expiry(&target, expired.scope(), expired.generation())
                .await
            {
                let _ = self.push.deliver(&target, prepared).await;
            }
        }
        if let Some(end_reason) = ended_reason {
            self.pending_ended_push_cleanup =
                Some(format!("subscription ended: {}", end_reason.as_str()));
            self.try_pending_held_push_cleanup().await;
        }
    }

    pub(super) async fn complete_drains(&self, records: &[ThreadSubscriptionRecord]) -> bool {
        // Drain state is derived from membership and remaining local windows.
        let _ = records;
        false
    }

    pub(super) async fn deliver_due(
        &mut self,
        due: &[ThreadId],
        facts: &HashMap<ThreadId, RootSubscriptionFacts>,
    ) -> Result<bool, BoardError> {
        self.allocate_due_follow(due, facts).await
    }

    pub(super) async fn select(
        &self,
        roots: &[ThreadId],
        maximum_root_notice_bytes: usize,
    ) -> Result<(SubscriptionBatch, SubscriptionBatchSettlement), BoardError> {
        let selected = self
            .board_store()?
            .lock()
            .await
            .select_project_subscription_notice(
                &self.reader,
                roots,
                self.clock.now(),
                maximum_root_notice_bytes,
                &self.pass_projects,
                &self.pass_preventions,
            )
            .await?;
        #[cfg(test)]
        self.observe(OwnerObservation::Selected);
        Ok(selected)
    }

    pub(super) async fn settle(
        &mut self,
        settlement: &SubscriptionBatchSettlement,
        outcome: SubscriptionDeliveryOutcome,
    ) -> Result<(), BoardError> {
        self.complete_batch_write(settlement, outcome).await?;
        #[cfg(test)]
        self.observe(OwnerObservation::Settled);
        Ok(())
    }

    async fn complete_batch_write(
        &mut self,
        settlement: &SubscriptionBatchSettlement,
        outcome: SubscriptionDeliveryOutcome,
    ) -> Result<(), BoardError> {
        let mut delay = Duration::from_secs(1);
        loop {
            let result = {
                let mut store = self.board_store()?.lock().await;
                let operation = WindowWrite::Settle(outcome.clone());
                store
                    .write_project_batch(
                        &self.reader,
                        settlement,
                        operation,
                        &self.pass_projects,
                        &self.pass_preventions,
                    )
                    .await
            };
            match result {
                Ok(()) => {
                    self.request_scan(ReaderScanScope::Projects(
                        self.pass_projects.iter().cloned().collect(),
                    ));
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(%error, "selected subscription batch write failed; retaining selection for retry")
                }
            }
            #[cfg(test)]
            self.observe(OwnerObservation::SettlementRetry);
            let deadline = self
                .clock
                .monotonic_now()
                .checked_add(delay)
                .ok_or_else(BoardError::board_unavailable)?;
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    self.release(settlement).await;
                    return Err(BoardError::board_unavailable());
                },
                () = self.clock.sleep_until(deadline) => {}
            }
            delay = delay.saturating_mul(2).min(Duration::from_secs(30));
        }
    }

    pub(super) async fn release(&self, settlement: &SubscriptionBatchSettlement) {
        let Ok(store) = self.board_store() else {
            return;
        };
        if let Err(error) = store
            .lock()
            .await
            .write_project_batch(
                &self.reader,
                settlement,
                WindowWrite::Release,
                &self.pass_projects,
                &self.pass_preventions,
            )
            .await
        {
            tracing::warn!(%error, "unhanded subscription selection could not be released");
        }
    }
}
