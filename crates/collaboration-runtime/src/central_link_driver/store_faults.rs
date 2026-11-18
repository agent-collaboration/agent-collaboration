//! A store that fails one named operation once (test only): the transaction
//! is never started, so the database is unchanged, exactly like a rolled-back
//! write that hit a busy database.
use crate::outbox_storage::{OutboxFuture, OutboxStorage};
use crate::outbox_store::{NewOutboxItem, OutboxInsert, OutboxStoreError, StoredOutboxWrite};
use central_link_client::{MemberOutcome, OutgoingWrite, PendingWrite, RequestStatus};
use runtime_primitives::SyncTime;
use std::sync::{Arc, Mutex as StdMutex};
use workspace_model::{Caller, RequestId};

/// Which operations fail next, once each.
#[derive(Clone, Default)]
pub(crate) struct FaultPlan(Arc<StdMutex<Vec<&'static str>>>);

impl FaultPlan {
    pub(crate) fn fail_once(&self, operation: &'static str) {
        self.0.lock().unwrap().push(operation);
    }
    fn take(&self, operation: &'static str) -> bool {
        let mut plan = self.0.lock().unwrap();
        match plan.iter().position(|planned| *planned == operation) {
            Some(index) => {
                plan.remove(index);
                true
            }
            None => false,
        }
    }
    /// The store is healthy again: no planned fault fires.
    pub(crate) fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    pub(crate) fn pending(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}

pub(crate) struct FaultyOutbox {
    pub(crate) inner: Box<dyn OutboxStorage>,
    pub(crate) plan: FaultPlan,
}

fn injected<T>() -> Result<T, OutboxStoreError> {
    Err(OutboxStoreError::Database(sqlx::Error::PoolTimedOut))
}

macro_rules! faulty {
    ($self:ident, $name:literal, $call:expr) => {
        if $self.plan.take($name) {
            Box::pin(async { injected() })
        } else {
            $call
        }
    };
}

impl OutboxStorage for FaultyOutbox {
    fn insert<'call>(
        &'call mut self,
        item: &'call NewOutboxItem,
        now: SyncTime,
    ) -> OutboxFuture<'call, OutboxInsert> {
        faulty!(self, "insert", self.inner.insert(item, now))
    }
    fn request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        request: &'call RequestId,
    ) -> OutboxFuture<'call, RequestStatus> {
        faulty!(
            self,
            "request_status",
            self.inner.request_status(caller, request)
        )
    }
    fn pending_for<'call>(
        &'call mut self,
        caller: &'call Caller,
    ) -> OutboxFuture<'call, Vec<PendingWrite>> {
        faulty!(self, "pending_for", self.inner.pending_for(caller))
    }
    fn outgoing_for<'call>(
        &'call mut self,
        caller: &'call Caller,
        limit: u16,
    ) -> OutboxFuture<'call, Vec<OutgoingWrite>> {
        faulty!(self, "outgoing_for", self.inner.outgoing_for(caller, limit))
    }
    fn delete_ended_before_retention(&mut self, now: SyncTime) -> OutboxFuture<'_, u64> {
        faulty!(
            self,
            "delete_ended_before_retention",
            self.inner.delete_ended_before_retention(now)
        )
    }
    fn waiting(&mut self) -> OutboxFuture<'_, Vec<(SyncTime, RequestId)>> {
        faulty!(self, "waiting", self.inner.waiting())
    }
    fn earliest_waiting(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        faulty!(self, "earliest_waiting", self.inner.earliest_waiting())
    }
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, Vec<StoredOutboxWrite>> {
        faulty!(
            self,
            "claim_next_batch",
            self.inner.claim_next_batch(mark, limit, now)
        )
    }
    fn sent_without_answer(&mut self) -> OutboxFuture<'_, Vec<RequestId>> {
        faulty!(
            self,
            "sent_without_answer",
            self.inner.sent_without_answer()
        )
    }
    fn mark_sent<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<StoredOutboxWrite>> {
        faulty!(self, "mark_sent", self.inner.mark_sent(requests, now))
    }
    fn record_outcome<'call>(
        &'call mut self,
        request: &'call RequestId,
        outcome: &'call MemberOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        faulty!(
            self,
            "record_outcome",
            self.inner.record_outcome(request, outcome, now)
        )
    }
    fn return_unapplied<'call>(
        &'call mut self,
        request: &'call RequestId,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        faulty!(
            self,
            "return_unapplied",
            self.inner.return_unapplied(request, now)
        )
    }
    fn return_unanswered<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        faulty!(
            self,
            "return_unanswered",
            self.inner.return_unanswered(requests, now)
        )
    }
}
