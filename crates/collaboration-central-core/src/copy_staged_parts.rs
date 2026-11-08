//! Central's one-day retention rule uses only the receiver's public inspection and discard APIs.
//!
//! Each staged transaction is its own due item. Its attempt subject is the
//! UUIDv7 key of its first staged part, so a transaction whose expiry fails
//! backs off alone while the Copy's other transactions still expire.
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use fleet_project_copy::{CopyInspection, StagedTransaction};
use fleet_protocol::TransactionKey;
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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

/// The attempt subject of a staged transaction: its first part's key.
fn subject(transaction: &StagedTransaction) -> Option<Uuid> {
    transaction.parts.first().map(|part| part.id)
}

/// Every staged transaction and its attempt subject, for one unit each.
pub(crate) async fn staged<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Vec<(TransactionKey, Uuid)>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    match fleet_project_copy::inspect_copy(scope).await {
        CopyInspection::Live { staged_parts, .. } => Ok(staged_parts
            .iter()
            .filter_map(|transaction| {
                subject(transaction).map(|subject| (transaction.transaction_key, subject))
            })
            .collect()),
        CopyInspection::Damaged { detail } => Err(StoreError::backend(detail)),
        CopyInspection::Uninitialized | CopyInspection::Retired { .. } => Ok(Vec::new()),
    }
}

/// The next time any staged transaction is due: its part deadline, or its
/// retry time while its expiry is failing (a given-up one is never due).
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
    let staged_parts = match fleet_project_copy::inspect_copy(scope).await {
        CopyInspection::Live { staged_parts, .. } => staged_parts,
        CopyInspection::Damaged { detail } => return Err(StoreError::backend(detail)),
        CopyInspection::Uninitialized | CopyInspection::Retired { .. } => return Ok(None),
    };
    let mut next = None::<SyncTime>;
    for transaction in &staged_parts {
        let Some(subject) = subject(transaction) else {
            continue;
        };
        let due = match crate::work_attempts::schedule(
            scope,
            subject,
            crate::work_attempts::AttemptKind::PartExpiry,
        )
        .await?
        {
            crate::work_attempts::SubjectSchedule::Unscheduled => deadline(transaction)?,
            crate::work_attempts::SubjectSchedule::RetryAt(at) => Some(at),
            crate::work_attempts::SubjectSchedule::NotDue => None,
        };
        if let Some(at) = due {
            next = Some(next.map_or(at, |previous| previous.min(at)));
        }
    }
    Ok(next)
}

/// The earliest staged-part deadline, whatever its attempt row says.
pub(crate) async fn next_part_deadline<Scope: TransactionScope>(
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
    match fleet_project_copy::inspect_copy(scope).await {
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

/// Discard one staged transaction if it is past its 1-day deadline or behind
/// the fence, in the caller's transaction.
pub(crate) async fn expire<Scope: TransactionScope>(
    scope: &mut Scope,
    key: TransactionKey,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    match fleet_project_copy::inspect_copy(scope).await {
        CopyInspection::Live {
            status,
            staged_parts,
            ..
        } => {
            let Some(transaction) = staged_parts
                .iter()
                .find(|transaction| transaction.transaction_key == key)
            else {
                // Assembled or discarded since it was listed: nothing left to expire.
                return Ok(());
            };
            let fenced = transaction.transaction_key.epoch < status.fence;
            let expired = !fenced && deadline(transaction)?.is_some_and(|at| now >= at);
            if fenced || expired {
                fleet_project_copy::discard_staged_parts(scope, &transaction.transaction_key)
                    .await
                    .map_err(fleet_project_copy::receiver_rollback)?;
            }
            Ok(())
        }
        CopyInspection::Damaged { detail } => Err(StoreError::backend(detail)),
        CopyInspection::Uninitialized | CopyInspection::Retired { .. } => Ok(()),
    }
}
