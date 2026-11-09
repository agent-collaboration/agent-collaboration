//! Central handoff time is the start of sent custody, not the item's creation time.
use crate::StorageError;
use chrono::{DateTime, Utc};
use collaboration_protocol::{KnownStuckMail, StuckMailSummary};
use std::num::NonZeroU64;

pub(crate) async fn read_stuck_sent_mail(
    connection: &mut sqlx_turso::TursoConnection,
    now: DateTime<Utc>,
) -> Result<StuckMailSummary, StorageError> {
    let cutoff_ms = now
        .timestamp_millis()
        .checked_sub(5 * 60 * 1000)
        .ok_or(StorageError::InvalidStoredRecord)?;
    // Checked-query exception: SDK-owned tables expose neither production
    // enumeration nor handoff metadata. Actual SDK rows are exercised below.
    let rows = sqlx::query_as::<sqlx_turso::Turso, StoredStatusAge>(
        "SELECT record_kind, status_kind, typeof(handed_off_at) AS handoff_class, COUNT(*) AS total, COUNT(CASE WHEN status_kind='sent' AND typeof(handed_off_at)='integer' AND handed_off_at < ? THEN 1 END) AS count, MIN(CASE WHEN status_kind='sent' AND typeof(handed_off_at)='integer' AND handed_off_at < ? THEN handed_off_at END) AS oldest, MIN(CASE WHEN typeof(handed_off_at)='integer' THEN handed_off_at END) AS earliest_handoff FROM _fleet_sync_outbox_items GROUP BY record_kind, status_kind, typeof(handed_off_at)"
    ).bind(cutoff_ms).bind(cutoff_ms).fetch_all(connection).await.map_err(|error| match error {
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => StorageError::InvalidStoredRecord,
        error => StorageError::Database(error),
    })?;
    let mut total = 0_i64;
    let mut unknown_recovered = 0_i64;
    let mut oldest = None;
    for row in rows {
        match row.handoff_class.as_str() {
            "integer" if row.earliest_handoff.is_some_and(|at| at >= 0) => {}
            "null" if row.earliest_handoff.is_none() => {}
            _ => return Err(StorageError::InvalidStoredRecord),
        }
        match (row.record_kind.as_str(), row.status_kind.as_deref()) {
            ("unresolvable", None) if row.handoff_class == "null" => continue,
            ("item" | "recovered", Some("sent")) => {
                if row.handoff_class == "null" {
                    if row.record_kind != "recovered" {
                        return Err(StorageError::InvalidStoredRecord);
                    }
                    unknown_recovered = unknown_recovered
                        .checked_add(row.total)
                        .ok_or(StorageError::InvalidStoredRecord)?;
                } else {
                    total = total
                        .checked_add(row.count)
                        .ok_or(StorageError::InvalidStoredRecord)?;
                    if let Some(at) = row.oldest {
                        oldest = Some(oldest.map_or(at, |previous: i64| previous.min(at)));
                    }
                }
            }
            (
                "item" | "recovered",
                Some(
                    "queued" | "atRecipient" | "held" | "delivered" | "uncertain" | "expired"
                    | "expiredAtUpgrade" | "cancelled" | "refused",
                ),
            ) => {}
            _ => return Err(StorageError::InvalidStoredRecord),
        }
    }
    let known_stuck = match (total, oldest) {
        (0, None) => None,
        (count, Some(oldest)) => {
            let count = positive_count(count)?;
            let age_ms = now
                .timestamp_millis()
                .checked_sub(oldest)
                .and_then(|age| u64::try_from(age).ok())
                .ok_or(StorageError::InvalidStoredRecord)?;
            Some(KnownStuckMail {
                count,
                oldest_age_seconds: age_ms / 1000,
            })
        }
        _ => return Err(StorageError::InvalidStoredRecord),
    };
    if unknown_recovered != 0 {
        return Ok(StuckMailSummary::AgeUnknown {
            recovered_count: positive_count(unknown_recovered)?,
            known_stuck,
        });
    }
    Ok(match known_stuck {
        Some(known) => StuckMailSummary::Stuck {
            count: known.count,
            oldest_age_seconds: known.oldest_age_seconds,
        },
        None => StuckMailSummary::Empty,
    })
}

fn positive_count(count: i64) -> Result<NonZeroU64, StorageError> {
    u64::try_from(count)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(StorageError::InvalidStoredRecord)
}

#[derive(sqlx::FromRow)]
struct StoredStatusAge {
    record_kind: String,
    status_kind: Option<String>,
    handoff_class: String,
    total: i64,
    count: i64,
    oldest: Option<i64>,
    earliest_handoff: Option<i64>,
}
