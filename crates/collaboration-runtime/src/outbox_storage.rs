//! The storage the central link driver keeps its outbox in. Production is
//! machine_local.sqlite (MachineLocalStore through MacOutbox); simulations
//! use an in-memory store behind the `test-support` feature, so a store
//! call never leaves the simulated host (Lead A's call, 2026-11-18: sqlx's
//! worker thread turned real I/O latency into simulated time). One shared
//! contract suite holds both to the same behaviour.
use crate::MachineLocalStore;
use crate::outbox_store::{
    MacOutbox, NewOutboxItem, OutboxInsert, OutboxStoreError, StoredOutboxWrite,
};
use central_link_client::{MemberOutcome, OutgoingWrite, PendingWrite, RequestStatus};
use runtime_primitives::SyncTime;
use std::{future::Future, pin::Pin};
use workspace_model::{Caller, RequestId};

pub type OutboxFuture<'call, T> =
    Pin<Box<dyn Future<Output = Result<T, OutboxStoreError>> + Send + 'call>>;

/// Every outbox operation the driver makes, with MacOutbox's meaning.
pub trait OutboxStorage: Send {
    fn insert<'call>(
        &'call mut self,
        item: &'call NewOutboxItem,
        now: SyncTime,
    ) -> OutboxFuture<'call, OutboxInsert>;
    fn request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        request: &'call RequestId,
    ) -> OutboxFuture<'call, RequestStatus>;
    fn pending_for<'call>(
        &'call mut self,
        caller: &'call Caller,
    ) -> OutboxFuture<'call, Vec<PendingWrite>>;
    fn outgoing_for<'call>(
        &'call mut self,
        caller: &'call Caller,
        limit: u16,
    ) -> OutboxFuture<'call, Vec<OutgoingWrite>>;
    fn delete_ended_before_retention(&mut self, now: SyncTime) -> OutboxFuture<'_, u64>;
    fn waiting(&mut self) -> OutboxFuture<'_, Vec<(SyncTime, RequestId)>>;
    /// The earliest receipt mark among waiting writes.
    fn earliest_waiting(&mut self) -> OutboxFuture<'_, Option<SyncTime>>;
    /// Up to `limit` writes due by `mark`, oldest first by (created_at,
    /// request_id), moved to sent in one transaction.
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, Vec<StoredOutboxWrite>>;
    fn sent_without_answer(&mut self) -> OutboxFuture<'_, Vec<RequestId>>;
    fn mark_sent<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<StoredOutboxWrite>>;
    fn record_outcome<'call>(
        &'call mut self,
        request: &'call RequestId,
        outcome: &'call MemberOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()>;
    fn return_unapplied<'call>(
        &'call mut self,
        request: &'call RequestId,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()>;
    fn return_unanswered<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, ()>;
}

/// The production outbox: machine_local.sqlite, through MacOutbox. A separate
/// owner type so machine_local_store does not depend on outbox_store.
pub struct MachineLocalOutbox {
    store: MachineLocalStore,
}

impl MachineLocalOutbox {
    #[must_use]
    pub fn new(store: MachineLocalStore) -> Self {
        Self { store }
    }

    /// The store underneath, for callers that need MacOutbox directly.
    pub fn store_mut(&mut self) -> &mut MachineLocalStore {
        &mut self.store
    }
}

impl OutboxStorage for MachineLocalOutbox {
    fn insert<'call>(
        &'call mut self,
        item: &'call NewOutboxItem,
        now: SyncTime,
    ) -> OutboxFuture<'call, OutboxInsert> {
        Box::pin(async move { MacOutbox::new(&mut self.store).insert(item, now).await })
    }
    fn request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        request: &'call RequestId,
    ) -> OutboxFuture<'call, RequestStatus> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .request_status(caller, request)
                .await
        })
    }
    fn pending_for<'call>(
        &'call mut self,
        caller: &'call Caller,
    ) -> OutboxFuture<'call, Vec<PendingWrite>> {
        Box::pin(async move { MacOutbox::new(&mut self.store).pending_for(caller).await })
    }
    fn outgoing_for<'call>(
        &'call mut self,
        caller: &'call Caller,
        limit: u16,
    ) -> OutboxFuture<'call, Vec<OutgoingWrite>> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .outgoing_for(caller, limit)
                .await
        })
    }
    fn delete_ended_before_retention(&mut self, now: SyncTime) -> OutboxFuture<'_, u64> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .delete_ended_before_retention(now)
                .await
        })
    }
    fn waiting(&mut self) -> OutboxFuture<'_, Vec<(SyncTime, RequestId)>> {
        Box::pin(async move { MacOutbox::new(&mut self.store).waiting().await })
    }
    fn earliest_waiting(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        Box::pin(async move { MacOutbox::new(&mut self.store).earliest_waiting().await })
    }
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, Vec<StoredOutboxWrite>> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .claim_next_batch(mark, limit, now)
                .await
        })
    }
    fn sent_without_answer(&mut self) -> OutboxFuture<'_, Vec<RequestId>> {
        Box::pin(async move { MacOutbox::new(&mut self.store).sent_without_answer().await })
    }
    fn mark_sent<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<StoredOutboxWrite>> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .mark_sent(requests, now)
                .await
        })
    }
    fn record_outcome<'call>(
        &'call mut self,
        request: &'call RequestId,
        outcome: &'call MemberOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .record_outcome(request, outcome, now)
                .await
        })
    }
    fn return_unapplied<'call>(
        &'call mut self,
        request: &'call RequestId,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .return_unapplied(request, now)
                .await
        })
    }
    fn return_unanswered<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .return_unanswered(requests, now)
                .await
        })
    }
}
