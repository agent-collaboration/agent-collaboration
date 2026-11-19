//! A store that fails one named operation once (test only), in one of two
//! ways: before the call (the transaction never starts, like a rolled-back
//! write on a busy database) or after it committed (the change is kept but the
//! caller sees an error, like a lost commit acknowledgement). Every fault that
//! fires is recorded, so a test can require that each named fault fired.
use crate::outbox_storage::{OutboxFuture, OutboxStorage};
use crate::outbox_store::{NewOutboxItem, OutboxInsert, OutboxStoreError, StoredOutboxWrite};
use central_link_client::{MemberOutcome, OutgoingWrite, PendingWrite, RequestStatus};
use runtime_primitives::SyncTime;
use std::sync::{Arc, Mutex as StdMutex};
use workspace_model::{Caller, RequestId};

/// How a planned fault fails its operation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FaultKind {
    /// Nothing is written; the caller sees an error.
    BeforeCall,
    /// The operation commits; the caller still sees an error.
    AfterCommit,
}

#[derive(Default)]
struct Faults {
    planned: Vec<(&'static str, FaultKind)>,
    fired: Vec<(&'static str, FaultKind)>,
}

/// Which operations fail next, once each, and which faults have fired.
#[derive(Clone, Default)]
pub(crate) struct FaultPlan(Arc<StdMutex<Faults>>);

impl FaultPlan {
    pub(crate) fn fail_once(&self, operation: &'static str) {
        self.fail(operation, FaultKind::BeforeCall);
    }
    pub(crate) fn fail(&self, operation: &'static str, kind: FaultKind) {
        self.0.lock().unwrap().planned.push((operation, kind));
    }
    fn take(&self, operation: &'static str) -> Option<FaultKind> {
        let mut faults = self.0.lock().unwrap();
        let index = faults
            .planned
            .iter()
            .position(|(planned, _)| *planned == operation)?;
        let fault = faults.planned.remove(index);
        faults.fired.push(fault);
        Some(fault.1)
    }
    /// The store is healthy again: no planned fault fires.
    pub(crate) fn clear(&self) {
        self.0.lock().unwrap().planned.clear();
    }
    pub(crate) fn pending(&self) -> Vec<&'static str> {
        let faults = self.0.lock().unwrap();
        faults
            .planned
            .iter()
            .map(|(operation, _)| *operation)
            .collect()
    }
    /// Every fault that fired so far, in order.
    pub(crate) fn fired(&self) -> Vec<(&'static str, FaultKind)> {
        self.0.lock().unwrap().fired.clone()
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
        match $self.plan.take($name) {
            None => $call,
            Some(FaultKind::BeforeCall) => Box::pin(async { injected() }),
            Some(FaultKind::AfterCommit) => {
                let call = $call;
                Box::pin(async move {
                    let _committed = call.await;
                    injected()
                })
            }
        }
    };
}

impl OutboxStorage for FaultyOutbox {
    fn insert<'call>(
        &'call mut self,
        item: &'call NewOutboxItem,
        now: SyncTime,
    ) -> OutboxFuture<'call, OutboxInsert> {
        match self.plan.take("insert") {
            None => self.inner.insert(item, now),
            Some(FaultKind::BeforeCall) => Box::pin(async {
                Err(OutboxStoreError::InsertNotStarted(
                    sqlx::Error::PoolTimedOut,
                ))
            }),
            Some(FaultKind::AfterCommit) => {
                let call = self.inner.insert(item, now);
                Box::pin(async move {
                    let _committed = call.await;
                    injected()
                })
            }
        }
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
    fn earliest_deadline(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        faulty!(self, "earliest_deadline", self.inner.earliest_deadline())
    }
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, crate::ClaimedBatch> {
        faulty!(
            self,
            "claim_next_batch",
            self.inner.claim_next_batch(mark, limit, now)
        )
    }
    fn end_overdue(
        &mut self,
        now: SyncTime,
    ) -> OutboxFuture<'_, Vec<central_link_client::EndedWrite>> {
        faulty!(self, "end_overdue", self.inner.end_overdue(now))
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
    ) -> OutboxFuture<'call, Vec<central_link_client::EndedWrite>> {
        faulty!(
            self,
            "return_unanswered",
            self.inner.return_unanswered(requests, now)
        )
    }
}
