//! Checkpoint-before-prefix expiry, atomically preserving remembered addresses.
use crate::{JournalError, JournalPosition, ObservationJournal};
use chrono::{DateTime, Months, Utc};
use collaboration_protocol::{
    AddressEntry, LifecycleObservation, LifecycleSubject, ObservationTimestamp,
};
use sqlx::{Connection, Row};

/// Retains lifecycle observations for two calendar months.
pub(crate) const LIFECYCLE_RECORD_RETENTION_MONTHS: u32 = 2;

/// Retains checkpointed addresses for two calendar months after their last observation.
pub(crate) const CHECKPOINT_ADDRESS_RETENTION_MONTHS: u32 = 2;

pub(crate) fn observation_timestamp_millis(
    timestamp: &ObservationTimestamp,
) -> Result<i64, JournalError> {
    let timestamp = String::from(timestamp.clone());
    DateTime::parse_from_rfc3339(&timestamp)
        .map(|parsed| parsed.timestamp_millis())
        .map_err(|_| JournalError::InvalidStorage)
}

pub(crate) fn decode_checkpoint_address(
    address_key: &str,
    encoded: &str,
    last_seen_ms: i64,
) -> Result<AddressEntry, JournalError> {
    let entry: AddressEntry =
        serde_json::from_str(encoded).map_err(|_| JournalError::InvalidCheckpointAddress {
            address_key: address_key.to_owned(),
        })?;
    let encoded_key = serde_json::to_string(&entry.address).map_err(|_| {
        JournalError::InvalidCheckpointAddress {
            address_key: address_key.to_owned(),
        }
    })?;
    let observed_at_ms = observation_timestamp_millis(&entry.last_observed_at).map_err(|_| {
        JournalError::InvalidCheckpointAddress {
            address_key: address_key.to_owned(),
        }
    })?;
    if last_seen_ms == 0 || encoded_key != address_key || observed_at_ms != last_seen_ms {
        return Err(JournalError::InvalidCheckpointAddress {
            address_key: address_key.to_owned(),
        });
    }
    Ok(entry)
}

fn retention_cutoff_millis(now_seconds: i64, months: u32) -> Result<i64, JournalError> {
    let now = DateTime::<Utc>::from_timestamp(now_seconds, 0).ok_or(JournalError::InvalidRecord)?;
    now.checked_sub_months(Months::new(months))
        .map(|cutoff| cutoff.timestamp_millis())
        .ok_or(JournalError::InvalidRecord)
}

impl ObservationJournal {
    /// Applies rolling two-calendar-month expiry while retaining the address checkpoint.
    /// Returns the checkpoint sequence, including when no additional records expire.
    pub async fn expire_history(&mut self, now: i64) -> Result<u64, JournalError> {
        if now < 0 {
            return Err(JournalError::InvalidRecord);
        }
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let clock: i64 =
            sqlx::query_scalar("SELECT retention_clock FROM journal_metadata WHERE singleton=1")
                .fetch_one(&mut *transaction)
                .await?;
        let clock = clock.max(now);
        let lifecycle_cutoff = retention_cutoff_millis(clock, LIFECYCLE_RECORD_RETENTION_MONTHS)?;
        let address_cutoff = retention_cutoff_millis(clock, CHECKPOINT_ADDRESS_RETENTION_MONTHS)?;
        let lifecycle_cutoff_seconds = lifecycle_cutoff.div_euclid(1_000);
        let old: i64 =
            sqlx::query_scalar("SELECT sequence FROM journal_checkpoint WHERE singleton=1")
                .fetch_one(&mut *transaction)
                .await?;
        let eligible: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(sequence) FROM lifecycle_records WHERE retention_at < ?",
        )
        .bind(lifecycle_cutoff_seconds)
        .fetch_one(&mut *transaction)
        .await?;
        let end = eligible.unwrap_or(old);
        let mut cursor = old;
        while cursor < end {
            let records=sqlx::query("SELECT sequence,observation_json FROM lifecycle_records WHERE sequence>? AND sequence<=? ORDER BY sequence LIMIT 100").bind(cursor).bind(end).fetch_all(&mut *transaction).await?;
            if records.is_empty() {
                return Err(JournalError::InvalidStorage);
            }
            for row in records {
                let sequence: i64 = row.try_get("sequence")?;
                if sequence != cursor + 1 {
                    return Err(JournalError::InvalidStorage);
                }
                let text: String = row.try_get("observation_json")?;
                let observation: LifecycleObservation =
                    serde_json::from_str(&text).map_err(|_| JournalError::InvalidStorage)?;
                observation
                    .validate()
                    .map_err(|_| JournalError::InvalidStorage)?;
                if let LifecycleSubject::Thread { address } = &observation.subject {
                    let key =
                        serde_json::to_string(address).map_err(|_| JournalError::InvalidStorage)?;
                    let prior: Option<(String, i64)> = sqlx::query_as(
                        "SELECT entry_json,last_seen_ms FROM checkpoint_addresses WHERE address_key=?",
                    )
                    .bind(&key)
                    .fetch_optional(&mut *transaction)
                    .await?;
                    let prior = prior
                        .map(|(encoded, last_seen_ms)| {
                            decode_checkpoint_address(&key, &encoded, last_seen_ms)
                        })
                        .transpose()?;
                    let entry = crate::thread_address_book::reduce_address_observation(
                        prior,
                        &observation,
                        JournalPosition {
                            journal_id: self.journal_id.clone(),
                            sequence: u64::try_from(sequence)
                                .map_err(|_| JournalError::InvalidStorage)?,
                        },
                    )?;
                    let last_seen_ms = observation_timestamp_millis(&entry.last_observed_at)?;
                    if last_seen_ms == 0 {
                        return Err(JournalError::InvalidCheckpointAddress { address_key: key });
                    }
                    let entry =
                        serde_json::to_string(&entry).map_err(|_| JournalError::InvalidStorage)?;
                    sqlx::query("INSERT INTO checkpoint_addresses(address_key,entry_json,last_seen_ms) VALUES (?,?,?) ON CONFLICT(address_key) DO UPDATE SET entry_json=excluded.entry_json,last_seen_ms=excluded.last_seen_ms")
                        .bind(key)
                        .bind(entry)
                        .bind(last_seen_ms)
                        .execute(&mut *transaction)
                        .await?;
                }
                cursor = sequence;
            }
        }
        let removed:i64=sqlx::query_scalar("SELECT COALESCE(SUM(length(CAST(observation_json AS BLOB))),0) FROM lifecycle_records WHERE sequence<=?").bind(end).fetch_one(&mut *transaction).await?;
        sqlx::query("UPDATE journal_checkpoint SET sequence=? WHERE singleton=1")
            .bind(end)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM lifecycle_records WHERE sequence<=?")
            .bind(end)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE journal_metadata SET retention_clock=?,payload_bytes=payload_bytes-? WHERE singleton=1")
            .bind(clock)
            .bind(removed)
            .execute(&mut *transaction)
            .await?;
        prune_stale_checkpoint_addresses(&mut transaction, address_cutoff).await?;
        transaction.commit().await?;
        u64::try_from(end).map_err(|_| JournalError::InvalidStorage)
    }
}

async fn prune_stale_checkpoint_addresses(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    cutoff_ms: i64,
) -> Result<(), JournalError> {
    let mut cursor = String::new();
    loop {
        let rows=sqlx::query("SELECT address_key,entry_json,last_seen_ms FROM checkpoint_addresses WHERE last_seen_ms<? AND address_key>? ORDER BY address_key LIMIT 100")
            .bind(cutoff_ms)
            .bind(&cursor)
            .fetch_all(&mut **transaction)
            .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let key: String = row.try_get("address_key")?;
            cursor.clone_from(&key);
            let encoded: String = row.try_get("entry_json")?;
            let last_seen_ms: i64 = row.try_get("last_seen_ms")?;
            decode_checkpoint_address(&key, &encoded, last_seen_ms)?;
            if let Some(current) = sqlx::query_scalar::<_, String>(
                "SELECT entry_json FROM thread_addresses WHERE address_key=?",
            )
            .bind(&key)
            .fetch_optional(&mut **transaction)
            .await?
            {
                let current: AddressEntry =
                    serde_json::from_str(&current).map_err(|_| JournalError::InvalidStorage)?;
                if serde_json::to_string(&current.address)
                    .map_err(|_| JournalError::InvalidStorage)?
                    != key
                {
                    return Err(JournalError::InvalidStorage);
                }
                if observation_timestamp_millis(&current.last_observed_at)? < cutoff_ms {
                    sqlx::query("DELETE FROM thread_addresses WHERE address_key=?")
                        .bind(&key)
                        .execute(&mut **transaction)
                        .await?;
                }
            }
            sqlx::query("DELETE FROM checkpoint_addresses WHERE address_key=? AND last_seen_ms<?")
                .bind(key)
                .bind(cutoff_ms)
                .execute(&mut **transaction)
                .await?;
        }
    }
    Ok(())
}
