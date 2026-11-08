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
    /// A Copy, with its DatabaseId as the subject of its object-level failures.
    Copy {
        subject: durable_steps::Uuid,
    },
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

/// One source's next-due read, kept until every source has been read.
struct SourceRead {
    source: &'static str,
    read: Result<Option<SyncTime>, StoreError>,
}

impl SourceRead {
    /// A failed read counts as due at `retry_at` (none once its row is given
    /// up), so one corrupt row delays only its own source.
    fn next(self, retry_at: Option<SyncTime>) -> Option<SyncTime> {
        self.read.unwrap_or(retry_at)
    }
}

async fn read_source<Read>(source: &'static str, read: Read) -> SourceRead
where
    Read: std::future::Future<Output = Result<Option<SyncTime>, StoreError>>,
{
    SourceRead {
        source,
        read: read.await,
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
    let attempt_id = runtime_primitives::IdSource::new_id(platform.io().as_ref());
    let written = platform
        .store()
        .transaction(move |scope| {
            Box::pin(async move {
                use durable_steps::storage::JournalStorage;
                let subject = match sources {
                    DueSources::Fleet => crate::fleet_feed_store::state_subject(scope).await.ok(),
                    DueSources::Copy { subject } => Some(subject),
                };
                // This transaction writes the alarm; a failure rolls this back and
                // records alarm_write afresh. Cleared first so the old row's retry
                // time is not read back as due work.
                if let Some(subject) = subject {
                    crate::work_attempts::succeeded(
                        scope,
                        subject,
                        crate::work_attempts::AttemptKind::AlarmWrite,
                    )
                    .await?;
                }
                // Deadlines the cadence never holds: forward expiry is a protocol
                // answer (#scheduler), and until slice 4 the engine still carries
                // request and owed jobs. Every other source is held by the cadence.
                let journal = read_source("long_operations", scope.earliest()).await;
                let forwards = match sources {
                    DueSources::Fleet => Some(
                        read_source(
                            "forward_expiry",
                            crate::project_request_store::next_forward_expiry(scope),
                        )
                        .await,
                    ),
                    DueSources::Copy { .. } => None,
                };
                let held = match sources {
                    DueSources::Fleet => vec![
                        read_source(
                            "notice_expiry",
                            crate::machine_notice_store::next_expiry_any(scope),
                        )
                        .await,
                        read_source(
                            "feed_retention",
                            crate::fleet_feed_store::next_retention_at(scope),
                        )
                        .await,
                        read_source("push_expiry", async {
                            fleet_mail_delivery::Outbox::<_, board_sync_protocol::BoardDeliveryTypes, _>::new(
                                scope,
                                crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                                &crate::fleet_sender_binding::FleetSenderBinding,
                            )
                            .next_push_expiry()
                            .await
                            .map_err(StoreError::backend)
                        })
                        .await,
                        read_source("mail_retention", async {
                            fleet_mail_delivery::next_relay_record_removal(scope)
                                .await
                                .map_err(StoreError::backend)
                        })
                        .await,
                    ],
                    DueSources::Copy { .. } => vec![
                        read_source(
                            "staged_part_expiry",
                            crate::copy_staged_parts::next_expiry(scope),
                        )
                        .await,
                    ],
                };
                // A failed read is kept on the object's source_read row, which
                // backs off; all reads succeeding forgets it.
                let failed = [Some(&journal), forwards.as_ref()]
                    .into_iter()
                    .flatten()
                    .chain(held.iter())
                    .find_map(|source| {
                        source.read.as_ref().err().map(|failure| (source.source, failure))
                    });
                let fallback = next_batch_at.unwrap_or(now).max(now);
                let retry_at = match (failed, subject) {
                    (Some((source, failure)), Some(subject)) => {
                        eprintln!("central due source {source} could not report its next time: {failure}");
                        let mut record = crate::work_attempts::AttemptFailure::from(failure);
                        record.message = format!("{source}: {}", record.message);
                        match crate::work_attempts::failed(
                            scope,
                            attempt_id,
                            subject,
                            crate::work_attempts::AttemptKind::SourceRead,
                            &record,
                            now,
                        )
                        .await
                        {
                            Ok(crate::work_attempts::AttemptOutcome::Retry { at }) => Some(at),
                            // Given up: the read is no longer retried on its own;
                            // any other wake still reads it.
                            Ok(crate::work_attempts::AttemptOutcome::GivenUp) => None,
                            Err(unwritten) => {
                                eprintln!("central source_read row could not be written: {unwritten}");
                                Some(fallback)
                            }
                        }
                    }
                    (Some((source, failure)), None) => {
                        eprintln!("central due source {source} could not report its next time, and the object has no subject: {failure}");
                        Some(fallback)
                    }
                    (None, Some(subject)) => {
                        crate::work_attempts::succeeded(
                            scope,
                            subject,
                            crate::work_attempts::AttemptKind::SourceRead,
                        )
                        .await?;
                        None
                    }
                    (None, None) => None,
                };
                // Read only after source_read and alarm_write are settled, so a
                // row just cleared is not counted as due.
                let retries =
                    read_source("attempt_retries", crate::work_attempts::next_retry(scope))
                        .await;
                if let Err(failure) = &retries.read {
                    eprintln!("central attempt retries could not be read: {failure}");
                }
                let retry_at = retry_at.or(retries.read.is_err().then_some(fallback));
                let deadlines = earliest([
                    journal.next(retry_at),
                    forwards.and_then(|forwards| forwards.next(retry_at)),
                ]);
                let next_due = earliest(
                    held.into_iter()
                        .map(|source| source.next(retry_at))
                        .chain([retries.next(retry_at)]),
                );
                let alarm = earliest([deadlines, next_alarm(next_due, next_batch_at)]);
                alarm_owner
                    .alarm()
                    .set_exact(alarm)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(alarm)
            })
        })
        .await;
    if let Err(failure) = &written {
        record_alarm_write_failure(platform, sources, now, failure).await;
    }
    written
}

/// The alarm write failed and its transaction rolled back: keep the failure on
/// the object's alarm_write row, in a transaction of its own.
async fn record_alarm_write_failure<P: ObjectPlatform>(
    platform: &Arc<P>,
    sources: DueSources,
    now: SyncTime,
    failure: &StoreError,
) where
    for<'transaction> <P::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = P::Database>,
    <P::Database as Database>::Arguments: IntoArguments<P::Database>,
    for<'value> String:
        Type<P::Database> + Encode<'value, P::Database> + Decode<'value, P::Database>,
    for<'value> Option<String>:
        Type<P::Database> + Encode<'value, P::Database> + Decode<'value, P::Database>,
    for<'row> &'row str: ColumnIndex<<P::Database as Database>::Row>,
{
    let attempt_id = runtime_primitives::IdSource::new_id(platform.io().as_ref());
    let record = crate::work_attempts::AttemptFailure::from(failure);
    let recorded = platform
        .store()
        .transaction(move |scope| {
            Box::pin(async move {
                let subject = match sources {
                    DueSources::Fleet => crate::fleet_feed_store::state_subject(scope).await?,
                    DueSources::Copy { subject } => subject,
                };
                crate::work_attempts::failed(
                    scope,
                    attempt_id,
                    subject,
                    crate::work_attempts::AttemptKind::AlarmWrite,
                    &record,
                    now,
                )
                .await
            })
        })
        .await;
    if let Err(unwritten) = recorded {
        eprintln!("central alarm_write row could not be written: {unwritten}");
    }
}

#[cfg(test)]
#[path = "alarm_scheduler_test.rs"]
mod tests;
