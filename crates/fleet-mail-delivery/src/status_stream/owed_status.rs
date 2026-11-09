//! Where a status publication owed to an item's sender is recorded.
//!
//! Receipts, reports and deadline expiry owe the sender a status publication
//! in the same transaction as their custody write. The Router-side relay passes
//! [`JournaledStatus`], which admits the existing publication task exactly as
//! before; central passes its own sink, which records an owed row instead.

use super::PublicationRef;
use crate::DeliveryError;
use durable_steps::{JournalScope, Task, storage::TransactionScope};
use fleet_protocol::{DeliveryTypes, SyncTime};
use std::{future::Future, marker::PhantomData};

/// Records a status publication owed to the item's sender.
pub trait OwedStatus<S, D: DeliveryTypes> {
    fn owe_status<'call>(
        scope: &'call mut S,
        reference: &'call PublicationRef<D::StatusRoute>,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call;
}

/// Owes a status publication by admitting `TPublish`.
pub struct JournaledStatus<TPublish>(PhantomData<fn() -> TPublish>);

impl<S, D, TPublish> OwedStatus<S, D> for JournaledStatus<TPublish>
where
    S: TransactionScope + JournalScope,
    D: DeliveryTypes,
    TPublish: Task<Input = PublicationRef<D::StatusRoute>>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "explicit Send avoids async-fn/GAT lifetime generality errors in journal transactions"
    )]
    fn owe_status<'call>(
        scope: &'call mut S,
        reference: &'call PublicationRef<D::StatusRoute>,
        _now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        async move {
            scope
                .admit::<TPublish>(
                    &crate::status_publication::publication_request_key(reference)?,
                    reference,
                )
                .await
                .map(|_| ())
                .map_err(|error| DeliveryError::Storage(error.to_string()))
        }
    }
}
