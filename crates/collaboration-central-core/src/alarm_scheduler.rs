//! The object's alarm owner: one recompute after every event sets it exactly.
//!
//! Units only lower the alarm inside their own transactions. The recompute is
//! the only exact write. It reads every due source and writes the alarm in one
//! transaction, so a unit committed by another event between our read and our
//! write cannot be overwritten by a stale value.

mod batch_claim;

pub(crate) use batch_claim::{BatchClaim, ClaimDeclined};

use crate::{ObjectAlarm, ObjectPlatform};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

/// Which object's due sources the recompute reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DueSources {
    Fleet,
    Copy,
}

/// The alarm after an event: the earliest due time, never earlier than the
/// cadence allows the next batch, and absent when nothing is due.
pub(crate) fn next_alarm(
    next_due: Option<SyncTime>,
    next_batch_at: Option<SyncTime>,
) -> Option<SyncTime> {
    next_due.map(|due| match next_batch_at {
        Some(floor) if due < floor => floor,
        _ => due,
    })
}

fn earliest(times: impl IntoIterator<Item = Option<SyncTime>>) -> Option<SyncTime> {
    times.into_iter().flatten().min()
}

/// A source whose next-due read fails counts as due at the next batch slot, so
/// one corrupt row delays only its own source and never removes the wake.
fn contained(
    source: &str,
    read: Result<Option<SyncTime>, StoreError>,
    retry_at: SyncTime,
) -> Option<SyncTime> {
    match read {
        Ok(next) => next,
        Err(failure) => {
            eprintln!("central due source {source} could not report its next time: {failure}");
            Some(retry_at)
        }
    }
}

/// Read every due source of `sources` and set the alarm exactly, in one transaction.
pub(crate) async fn recompute<P: ObjectPlatform>(
    platform: &Arc<P>,
    sources: DueSources,
    now: SyncTime,
    next_batch_at: Option<SyncTime>,
) -> Result<Option<SyncTime>, StoreError>
where
    for<'transaction> <P::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = P::Database>,
    <P::Database as Database>::Arguments: IntoArguments<P::Database>,
    for<'value> String:
        Type<P::Database> + Encode<'value, P::Database> + Decode<'value, P::Database>,
    for<'value> Option<String>:
        Type<P::Database> + Encode<'value, P::Database> + Decode<'value, P::Database>,
    for<'row> &'row str: ColumnIndex<<P::Database as Database>::Row>,
{
    let alarm_owner = platform.clone();
    platform
        .store()
        .transaction(move |scope| {
            Box::pin(async move {
                use durable_steps::storage::JournalStorage;
                let retry_at = next_batch_at.unwrap_or(now).max(now);
                let journal = contained("long_operations", scope.earliest().await, retry_at);
                // Deadlines the cadence never holds: forward expiry is a protocol
                // answer (#scheduler), and until slice 4 the engine still carries
                // request and owed jobs. Every other source is held by the cadence.
                let deadlines = match sources {
                    DueSources::Fleet => earliest([
                        journal,
                        contained(
                            "forward_expiry",
                            crate::project_request_store::next_deadline(scope).await,
                            retry_at,
                        ),
                    ]),
                    DueSources::Copy => journal,
                };
                let next_due = match sources {
                    DueSources::Fleet => earliest([
                        contained(
                            "notice_expiry",
                            crate::machine_notice_store::next_expiry_any(scope).await,
                            retry_at,
                        ),
                        contained(
                            "feed_retention",
                            crate::fleet_feed_store::next_retention_at(scope).await,
                            retry_at,
                        ),
                    ]),
                    DueSources::Copy => earliest([contained(
                        "staged_part_expiry",
                        crate::copy_staged_parts::next_expiry(scope).await,
                        retry_at,
                    )]),
                };
                let alarm = earliest([deadlines, next_alarm(next_due, next_batch_at)]);
                alarm_owner
                    .alarm()
                    .set_exact(alarm)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(alarm)
            })
        })
        .await
}

#[cfg(test)]
#[path = "alarm_scheduler_test.rs"]
mod tests;
