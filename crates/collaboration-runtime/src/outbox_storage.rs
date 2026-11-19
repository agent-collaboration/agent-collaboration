//! The storage the central link driver keeps its outbox in. Production is
//! machine_local.sqlite (MachineLocalStore through MacOutbox); simulations
//! use an in-memory store behind the `test-support` feature, so a store
//! call never leaves the simulated host (Lead A's call, 2026-11-18: sqlx's
//! worker thread turned real I/O latency into simulated time). One shared
//! contract suite holds both to the same behaviour.
use crate::MachineLocalStore;
use crate::outbox_store::{
    ClaimedBatch, MacOutbox, NewOutboxItem, OutboxInsert, OutboxStoreError, StoredOutboxWrite,
};
use central_link_client::{EndedWrite, MemberOutcome, OutgoingWrite, PendingWrite, RequestStatus};
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
    fn scoped_request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        query: &'call central_link_protocol::RequestStatusQuery,
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
    /// The earliest expiry among waiting writes, even when the link is down.
    fn earliest_deadline(&mut self) -> OutboxFuture<'_, Option<SyncTime>>;
    /// Up to `limit` writes due by `mark` and before their deadline, oldest
    /// first by (created_at, request_id), moved to sent in one transaction
    /// that first ends every overdue waiting write (outbox_deadline).
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, ClaimedBatch>;
    /// Ends every waiting write past its deadline.
    fn end_overdue(&mut self, now: SyncTime) -> OutboxFuture<'_, Vec<EndedWrite>>;
    /// Recover the original acceptance and retain the typed late delivery fact.
    /// True means the caller's pending inbox handoff can notify its sink.
    fn record_sender_delivery<'call>(
        &'call mut self,
        caller: &'call Caller,
        notice: &'call central_link_client::SenderDeliveryOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, bool>;
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
    /// Sent writes back to waiting; one past its deadline ends instead.
    fn return_unanswered<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<EndedWrite>>;
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
    fn scoped_request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        query: &'call central_link_protocol::RequestStatusQuery,
    ) -> OutboxFuture<'call, RequestStatus> {
        Box::pin(async move {
            let Some(stored) = crate::outbox_store::read_decoded_outbox(
                &mut self.store.connection,
                &query.request_id,
            )
            .await?
            else {
                return Ok(RequestStatus::NotFound);
            };
            if stored.caller != *caller {
                return Ok(RequestStatus::NotFound);
            }
            use crate::outbox_store::StoredPayload;
            let scope = match &stored.payload {
                StoredPayload::Unanswered(write) => write.request_scope(),
                StoredPayload::Answered(profile) => profile.target.request_scope(),
            };
            Ok(if scope == query.scope {
                stored.status
            } else {
                RequestStatus::NotFound
            })
        })
    }
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
    fn earliest_deadline(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        Box::pin(async move {
            let waiting = crate::outbox_item_state::OutboxItemState::InOutbox.as_str();
            let answer_lost = crate::outbox_item_state::OutboxItemState::SentAnswerLost.as_str();
            let deadline = sqlx::query_scalar!(
                r#"SELECT min(expires_at_ms) AS "deadline?: i64" FROM outbox_items
                   WHERE item_state IN (?, ?)"#,
                waiting,
                answer_lost
            )
            .fetch_one(&mut self.store.connection)
            .await?;
            Ok(deadline.map(SyncTime::from_unix_millis))
        })
    }
    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, ClaimedBatch> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .claim_next_batch(mark, limit, now)
                .await
        })
    }
    fn end_overdue(&mut self, now: SyncTime) -> OutboxFuture<'_, Vec<EndedWrite>> {
        Box::pin(async move { MacOutbox::new(&mut self.store).end_overdue(now).await })
    }
    fn record_sender_delivery<'call>(
        &'call mut self,
        caller: &'call Caller,
        notice: &'call central_link_client::SenderDeliveryOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, bool> {
        Box::pin(
            crate::outbox_delivery_outcome_store::record_sender_delivery(
                &mut self.store,
                caller,
                notice,
                now,
            ),
        )
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
    ) -> OutboxFuture<'call, Vec<EndedWrite>> {
        Box::pin(async move {
            MacOutbox::new(&mut self.store)
                .return_unanswered(requests, now)
                .await
        })
    }
}
