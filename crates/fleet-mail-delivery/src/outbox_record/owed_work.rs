//! Where a submission's push and a record's removal are recorded as owed.
//!
//! Outbox submissions owe the recipient's push and the record's removal at
//! retention; a sender's settlement owes only the removal. They call these
//! sinks in the same transaction as the custody write. The Router-side relay
//! passes [`JournaledWork`] and [`JournaledRemoval`], which admit the existing
//! journal tasks exactly as before; central passes its own sink, which records
//! owed rows instead.

use super::PushInput;
use crate::DeliveryError;
use durable_steps::{
    JournalScope, Task,
    storage::{JournalStorage, TransactionScope},
};
use fleet_protocol::{ItemId, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{future::Future, marker::PhantomData};

/// Records the recipient's push for an item that was just queued.
pub trait OwedPush<S> {
    fn owe_push<'call>(
        scope: &'call mut S,
        input: &'call PushInput,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call;
}

/// Records the record's removal once its retention ends.
pub trait OwedRemoval<S> {
    fn owe_record_removal<'call>(
        scope: &'call mut S,
        item: ItemId,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call;
}

/// Owes a submission's push and removal by admitting `TPush` and `TRetention`.
pub struct JournaledWork<TPush, TRetention>(PhantomData<fn() -> (TPush, TRetention)>);

/// Owes a record's removal by admitting `TRetention`.
pub struct JournaledRemoval<TRetention>(PhantomData<fn() -> TRetention>);

impl<S, TPush, TRetention> OwedPush<S> for JournaledWork<TPush, TRetention>
where
    S: TransactionScope + JournalScope,
    TPush: Task<Input = PushInput>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "explicit Send avoids async-fn/GAT lifetime generality errors in journal transactions"
    )]
    fn owe_push<'call>(
        scope: &'call mut S,
        input: &'call PushInput,
        _now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        async move {
            scope
                .admit::<TPush>(&crate::delivery_request_key(&input.reference.item)?, input)
                .await
                .map(|_| ())
                .map_err(|error| DeliveryError::Storage(error.to_string()))
        }
    }
}

impl<S, TPush, TRetention> OwedRemoval<S> for JournaledWork<TPush, TRetention>
where
    JournaledRemoval<TRetention>: OwedRemoval<S>,
{
    fn owe_record_removal<'call>(
        scope: &'call mut S,
        item: ItemId,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        <JournaledRemoval<TRetention> as OwedRemoval<S>>::owe_record_removal(scope, item, now)
    }
}

impl<S, TRetention> OwedRemoval<S> for JournaledRemoval<TRetention>
where
    S: TransactionScope + JournalScope + JournalStorage,
    TRetention: Task<Input = ItemId>,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    fn owe_record_removal<'call>(
        scope: &'call mut S,
        item: ItemId,
        _now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        crate::admit_record_removal::<_, TRetention>(scope, item)
    }
}
