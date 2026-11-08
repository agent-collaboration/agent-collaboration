//! Central's one-day retention rule uses only the receiver's public inspection and discard APIs.
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_project_copy::{CopyInspection, StagedTransaction};
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

/// The next part deadline, or the Copy's retry time while expiry is failing.
pub(crate) async fn next_expiry<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: durable_steps::Uuid,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    match crate::work_attempts::schedule(
        scope,
        subject,
        crate::work_attempts::AttemptKind::PartExpiry,
    )
    .await?
    {
        crate::work_attempts::SubjectSchedule::Unscheduled => {}
        crate::work_attempts::SubjectSchedule::RetryAt(at) => return Ok(Some(at)),
        crate::work_attempts::SubjectSchedule::NotDue => return Ok(None),
    }
    next_part_deadline(scope).await
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

/// Discard staged parts past their 1-day deadline or behind the fence, in the
/// caller's transaction; answers the next deadline still pending.
pub(crate) async fn expire<Scope: TransactionScope>(
    scope: &mut Scope,
    now: SyncTime,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut next = None::<SyncTime>;
    match fleet_project_copy::inspect_copy(scope).await {
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
                    || at.is_some_and(|at| now >= at)
                {
                    fleet_project_copy::discard_staged_parts(scope, &transaction.transaction_key)
                        .await
                        .map_err(fleet_project_copy::receiver_rollback)?;
                } else if let Some(at) = at {
                    next = Some(next.map_or(at, |previous| previous.min(at)));
                }
            }
        }
        CopyInspection::Damaged { detail } => return Err(StoreError::backend(detail)),
        CopyInspection::Uninitialized | CopyInspection::Retired { .. } => {}
    }
    Ok(next)
}
