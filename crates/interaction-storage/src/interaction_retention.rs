//! One bounded transaction prunes approval routes past the interaction retention window.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};

/// The largest batch one prune pass may delete.
pub const MAX_INTERACTION_PRUNE_BATCH: u32 = 500;

impl InteractionStore {
    /// Deletes up to `batch_size` approval routes unused since the retention cutoff.
    pub async fn prune_interaction_records(
        &self,
        now: DateTime<Utc>,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        prune(self, now, batch_size).await
    }

    /// The first moment at or after `now` the interaction-record retention rule allows
    /// deleting a record held now, or `None` when none could ever go.
    pub async fn interaction_records_prune_due(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, StorageError> {
        prune_due(self, now).await
    }
}

/// The first moment the rule allows deleting any approval route held now.
async fn prune_due(
    store: &InteractionStore,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, StorageError> {
    let mut connection = store.writer.lock().await;
    let oldest = crate::approval_routes::oldest_approval_route_use_ms(&mut connection)
        .await?
        .map(|millis| {
            DateTime::<Utc>::from_timestamp_millis(millis).ok_or(StorageError::InvalidRecord)
        })
        .transpose()?;
    Ok(oldest.and_then(|stamped| {
        sqlite_storage_maintenance::first_deletable_at(
            stamped,
            crate::INTERACTION_RETENTION_MONTHS,
            now,
        )
    }))
}

async fn prune(
    store: &InteractionStore,
    now: DateTime<Utc>,
    batch_size: u32,
) -> Result<u64, StorageError> {
    if !(1..=MAX_INTERACTION_PRUNE_BATCH).contains(&batch_size) {
        return Err(StorageError::InvalidRecord);
    }
    let cutoff =
        sqlite_storage_maintenance::months_before(now, crate::INTERACTION_RETENTION_MONTHS)
            .ok_or(StorageError::InvalidRecord)?;
    let mut connection = store.writer.lock().await;
    let mut transaction = store
        .transaction(
            &mut connection,
            sqlite_storage_maintenance::TransactionStart::Immediate,
        )
        .await?;
    let pruned = crate::approval_routes::prune_expired(
        &mut transaction,
        cutoff.timestamp_millis(),
        i64::from(batch_size),
    )
    .await;
    let pruned = match pruned {
        Ok(pruned) => pruned,
        Err(error) => {
            transaction.rollback().await?;
            return Err(StorageError::Database(error));
        }
    };
    transaction.commit().await?;
    Ok(pruned)
}

#[cfg(test)]
#[path = "interaction_retention_test.rs"]
mod tests;
