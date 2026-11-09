//! One due item run as one unit (#units).
//!
//! The unit skips while the subject's attempt row says it isn't due, runs its
//! work and forgets the row in one transaction, or, when the work fails, records
//! the failure in a transaction of its own (the unit's rolled back). A failure
//! never propagates: the batch goes on with the next item.

use crate::work_attempts::{self, AttemptFailure, AttemptKind, AttemptOutcome};
use durable_steps::{AtomicStore, BoxFuture, StoreError, Uuid, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// The subject a due item's attempts are kept under.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DueItem {
    pub(crate) subject: Uuid,
    pub(crate) kind: AttemptKind,
    pub(crate) now: SyncTime,
    /// The attempt row's UUIDv7 if this run creates one.
    pub(crate) attempt_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DueUnitRun {
    /// The subject is backing off or given up.
    NotDue,
    Done,
    /// The work failed; `recorded` is what its attempt row now says, or `None`
    /// if even recording failed (logged only).
    Failed {
        failure: AttemptFailure,
        recorded: Option<AttemptOutcome>,
    },
}

pub(crate) async fn run_due_unit<Store, Driver, Work>(
    store: &Store,
    item: DueItem,
    work: Work,
) -> DueUnitRun
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
    Work: for<'scope, 'transaction> FnOnce(
            &'scope mut Store::Scope<'transaction>,
        ) -> BoxFuture<'scope, Result<(), StoreError>>
        + Send
        + 'static,
{
    let ran = store
        .transaction(move |scope| {
            Box::pin(async move {
                if !work_attempts::schedule(scope, item.subject, item.kind)
                    .await?
                    .allows(item.now)
                {
                    return Ok(false);
                }
                work(scope).await?;
                work_attempts::succeeded(scope, item.subject, item.kind).await?;
                Ok(true)
            })
        })
        .await;
    let failure = match ran {
        Ok(true) => return DueUnitRun::Done,
        Ok(false) => return DueUnitRun::NotDue,
        Err(failure) => AttemptFailure::from(&failure),
    };
    eprintln!(
        "central due item {} {} failed: {}",
        item.kind.as_str(),
        item.subject,
        failure.message
    );
    let record = failure.clone();
    let recorded = store
        .transaction(move |scope| {
            Box::pin(async move {
                work_attempts::failed(
                    scope,
                    item.attempt_id,
                    item.subject,
                    item.kind,
                    &record,
                    item.now,
                )
                .await
            })
        })
        .await
        .inspect_err(|error| eprintln!("central attempt row could not be written: {error}"))
        .ok();
    DueUnitRun::Failed { failure, recorded }
}
