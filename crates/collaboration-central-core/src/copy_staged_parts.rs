//! Central's one-day retention rule uses only the receiver's public inspection and discard APIs.
use crate::step_driver::{StepCallback, StepResult};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_receiver::{CopyInspection, StagedTransaction};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

// The legacy rule expires age > one day. Millisecond clocks first satisfy it
// one millisecond after that day; scheduling there avoids a due-now alarm loop.
const PART_RETENTION_MS: i64 = 86_400_001;

fn deadline(transaction: &StagedTransaction) -> Result<Option<SyncTime>, StoreError> {
    let mut next = None::<SyncTime>;
    for part in &transaction.parts {
        let at = part
            .staged_at
            .unix_millis()
            .checked_add(PART_RETENTION_MS)
            .ok_or_else(|| StoreError::backend("staged-part expiry exceeds the clock range"))?;
        let at = SyncTime::from_unix_millis(at);
        next = Some(next.map_or(at, |previous| previous.min(at)));
    }
    Ok(next)
}

pub(crate) async fn next_expiry<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    match fleet_sync_receiver::inspect_copy(scope).await {
        CopyInspection::Live { staged_parts, .. } => {
            let mut next = None::<SyncTime>;
            for transaction in staged_parts {
                if let Some(at) = deadline(&transaction)? {
                    next = Some(next.map_or(at, |previous| previous.min(at)));
                }
            }
            Ok(next)
        }
        CopyInspection::Damaged { detail } => Err(StoreError::backend(detail)),
        CopyInspection::Uninitialized | CopyInspection::Retired { .. } => Ok(None),
    }
}

pub(crate) struct ExpireStagedParts {
    pub now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ExpireStagedParts
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<SyncTime>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut next = None::<SyncTime>;
            match fleet_sync_receiver::inspect_copy(scope).await {
                CopyInspection::Live {
                    status,
                    staged_parts,
                    ..
                } => {
                    for transaction in staged_parts {
                        let at = if transaction.transaction_key.epoch < status.fence {
                            None
                        } else {
                            deadline(&transaction)?
                        };
                        if transaction.transaction_key.epoch < status.fence
                            || at.is_some_and(|at| self.now >= at)
                        {
                            fleet_sync_receiver::discard_staged_parts(
                                scope,
                                &transaction.transaction_key,
                            )
                            .await
                            .map_err(fleet_sync_receiver::receiver_rollback)?;
                        } else if let Some(at) = at {
                            next = Some(next.map_or(at, |previous| previous.min(at)));
                        }
                    }
                }
                CopyInspection::Damaged { detail } => return Err(StoreError::backend(detail)),
                CopyInspection::Uninitialized | CopyInspection::Retired { .. } => {}
            }
            Ok((next, Vec::new()))
        })
    }
}
