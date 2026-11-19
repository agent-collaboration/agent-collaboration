//! The driver's outbox flush. The store is the only truth for what waits, is
//! sent and is answered (Lead A, 2026-11-18): the schedule reducer says when
//! a batch may go, the store says what goes. A store error is not a link
//! event: it is logged, the link is left alone, and the next mark retries from
//! the store. Sent rows that are not on the wire are returned to waiting at
//! startup, on every Welcome and on any tick with no batch in flight, so a
//! failed write of an answer or a return is reconciled, never lost.
use super::{BatchInFlight, LinkDriver};
use central_link_client::{
    EndedWrite, FlushEffect, FlushEvent, FlushOutcome, FlushOutcomes, MAX_BATCH_WRITES,
    MemberOutcome, transition,
};
use central_link_protocol::{DataFrame, LinkFrame, LinkRequestBatch};
use runtime_primitives::SyncTime;
use std::collections::BTreeMap;
use workspace_model::RequestId;

impl LinkDriver {
    /// The store's earliest due work. A sent row with no batch on the wire
    /// is due too: the next tick reconciles it. A failed read assumes
    /// something is due, so the schedule arms the next mark and retries.
    pub(super) async fn earliest_due(&mut self) -> Option<SyncTime> {
        let due_now = Some(SyncTime::from_unix_millis(0));
        let idle = self.batch.is_none();
        let mut store = self.store.lock().await;
        if idle {
            match store.sent_without_answer().await {
                Ok(sent) if !sent.is_empty() => return due_now,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "outbox: reading sent writes failed; retrying next mark");
                    return due_now;
                }
            }
        }
        match store.earliest_waiting().await {
            Ok(earliest) => earliest,
            Err(error) => {
                tracing::warn!(%error, "outbox: reading the earliest waiting write failed; retrying next mark");
                due_now
            }
        }
    }

    /// Sent rows that are not on the wire go back to waiting; the next flush
    /// resends them under the same id and central answers from its request log.
    pub(super) async fn reconcile_sent(&mut self) {
        let on_the_wire: Vec<RequestId> = self
            .batch
            .as_ref()
            .map(|batch| batch.callers.keys().cloned().collect())
            .unwrap_or_default();
        let now = self.clock.now();
        let mut store = self.store.lock().await;
        let sent = match store.sent_without_answer().await {
            Ok(sent) => sent,
            Err(error) => {
                tracing::warn!(%error, "outbox: reading sent writes failed; reconciling next tick");
                return;
            }
        };
        let orphaned: Vec<RequestId> = sent
            .into_iter()
            .filter(|request| !on_the_wire.contains(request))
            .collect();
        if orphaned.is_empty() {
            return;
        }
        match store.return_unanswered(&orphaned, now).await {
            Ok(ended) => {
                drop(store);
                self.tell_ended(ended);
            }
            Err(error) => {
                tracing::warn!(%error, "outbox: returning sent writes failed; reconciling next tick");
            }
        }
    }

    /// Writes that passed their deadline go to the sink, so their agents hear.
    fn tell_ended(&self, ended: Vec<EndedWrite>) {
        if !ended.is_empty() {
            self.sink.ended(ended);
        }
    }

    /// A flush wake fired: reconcile when idle, end writes past their deadline,
    /// let the schedule decide, then prune writes that ended 60 days ago (a
    /// kept row is only a receipt).
    pub(super) async fn flush_tick(&mut self, now: SyncTime) {
        if self.batch.is_none() {
            self.reconcile_sent().await;
        }
        let overdue = self.store.lock().await.end_overdue(now).await;
        match overdue {
            Ok(ended) => self.tell_ended(ended),
            Err(error) => {
                tracing::warn!(%error, "outbox: ending overdue writes failed; it runs again next flush");
            }
        }
        let earliest_due = self.earliest_due().await;
        self.apply_flush(FlushEvent::Tick { now, earliest_due })
            .await;
        let mut store = self.store.lock().await;
        if let Err(error) = store.delete_ended_before_retention(now).await {
            tracing::warn!(%error, "outbox retention prune failed; it runs again next flush");
        }
    }

    /// The link welcomed this Router: reconcile, then let the schedule arm.
    pub(super) async fn flush_link_connected(&mut self) {
        self.reconcile_sent().await;
        let earliest_due = self.earliest_due().await;
        self.apply_flush(FlushEvent::LinkConnected { earliest_due })
            .await;
    }

    pub(super) async fn apply_flush(&mut self, event: FlushEvent) {
        let (state, effects) = match transition(self.flush.clone(), event) {
            Ok(applied) => applied,
            Err(error) => {
                tracing::warn!(%error, "outbox flush schedule refused an event");
                return;
            }
        };
        self.flush = state;
        for effect in effects {
            match effect {
                FlushEffect::SendBatch { mark } => Box::pin(self.send_batch(mark)).await,
                FlushEffect::WakeAt { mark } => {
                    let jitter = self.jitter.jitter_millis(mark).clamp(-3_000, 3_000);
                    let at = SyncTime::from_unix_millis(mark.unix_millis().saturating_add(jitter));
                    self.flush_wake = Some(self.clock.wall_deadline(at));
                }
            }
        }
        // pending_for calls wait for the batch on the wire to end.
        if self.batch.is_none() && !self.deferred_pending.is_empty() {
            for (caller, reply) in std::mem::take(&mut self.deferred_pending) {
                let pending = super::pending_for(&self.store, &caller).await;
                let _ = reply.send(pending);
            }
        }
    }

    /// Claims the next due writes from the store and sends them as one frame.
    async fn send_batch(&mut self, mark: SyncTime) {
        let now = self.clock.now();
        let limit = u16::try_from(MAX_BATCH_WRITES).unwrap_or(u16::MAX);
        let claimed = {
            let mut store = self.store.lock().await;
            store.claim_next_batch(mark, limit, now).await
        };
        let writes = match claimed {
            Ok(claimed) => {
                self.tell_ended(claimed.ended);
                if claimed.writes.is_empty() {
                    return self.end_batch().await;
                }
                claimed.writes
            }
            Err(error) => {
                // Not a link event: the claim rolled back; the next mark retries.
                tracing::warn!(%error, "outbox: claiming the batch failed; retrying next mark");
                return self.end_batch().await;
            }
        };
        let mut members = BTreeMap::new();
        let mut frames = Vec::with_capacity(writes.len());
        for write in &writes {
            if let Ok(LinkFrame::Data(DataFrame::Request { request })) = self
                .client
                .begin_request(write.caller.clone(), write.payload.clone())
            {
                members.insert(
                    request.correlation_id().clone(),
                    (write.request.clone(), write.caller.clone()),
                );
                frames.push(*request);
            }
        }
        let callers = writes
            .iter()
            .map(|write| (write.request.clone(), write.caller.clone()))
            .collect();
        self.batch = Some(BatchInFlight {
            mark,
            members,
            callers,
            outcomes: Vec::new(),
            returned: false,
        });
        if frames.len() != writes.len() {
            // The link went down after the schedule chose this mark: nothing
            // goes out and every claimed write returns to waiting.
            return self.return_batch().await;
        }
        let sent = match LinkRequestBatch::new(frames) {
            Ok(batch) => {
                self.send(&LinkFrame::Data(DataFrame::Batch { requests: batch }))
                    .await
            }
            Err(_) => false,
        };
        if !sent {
            self.socket_closed().await;
        }
    }

    /// Central answered one member: keep it, or return it to waiting when
    /// central did not apply it. A failed store write leaves the row sent for
    /// reconciliation.
    pub(super) async fn batch_member_answered(
        &mut self,
        request: RequestId,
        outcome: MemberOutcome,
    ) {
        let now = self.clock.now();
        let written = {
            let mut store = self.store.lock().await;
            if outcome.is_final() {
                store.record_outcome(&request, &outcome, now).await
            } else {
                store.return_unapplied(&request, now).await
            }
        };
        if let Err(error) = written {
            tracing::warn!(%error, "outbox: keeping a batch answer failed; reconciling next tick");
        }
        if let Some(batch) = self.batch.as_mut() {
            if outcome.is_final() {
                if let Some(caller) = batch.callers.get(&request).cloned() {
                    batch.outcomes.push(FlushOutcome {
                        caller,
                        request_id: request,
                        outcome,
                    });
                }
            } else {
                batch.returned = true;
            }
            if batch.members.is_empty() {
                let complete = !batch.returned;
                self.finish_batch(complete);
                self.end_batch().await;
            }
        }
    }

    /// The batch ends without its remaining answers (link lost or deadline):
    /// the unanswered members return to waiting.
    pub(super) async fn return_batch(&mut self) {
        let Some(batch) = self.batch.as_ref() else {
            return;
        };
        let unanswered: Vec<RequestId> = batch
            .members
            .values()
            .map(|(request, _)| request.clone())
            .collect();
        if !unanswered.is_empty() {
            let now = self.clock.now();
            let returned = self
                .store
                .lock()
                .await
                .return_unanswered(&unanswered, now)
                .await;
            match returned {
                Ok(ended) => self.tell_ended(ended),
                Err(error) => {
                    tracing::warn!(%error, "outbox: returning unanswered writes failed; reconciling next tick");
                }
            }
        }
        self.finish_batch(false);
        self.end_batch().await;
    }

    async fn end_batch(&mut self) {
        if self.flush.batch_in_flight() {
            let earliest_due = self.earliest_due().await;
            self.apply_flush(FlushEvent::BatchEnded { earliest_due })
                .await;
        }
    }

    fn finish_batch(&mut self, complete: bool) {
        if let Some(batch) = self.batch.take() {
            self.sink.deliver(FlushOutcomes {
                flush_at: batch.mark,
                outcomes: batch.outcomes,
                complete,
            });
        }
    }
}

#[cfg(test)]
#[path = "driver_deadline_test.rs"]
mod deadline_tests;

#[cfg(test)]
#[path = "custody_scenarios_test.rs"]
mod custody_scenarios;
