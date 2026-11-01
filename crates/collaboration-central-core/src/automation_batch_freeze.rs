//! The cutoff stores immutable bytes and immediate notice debt in the same Fleet transaction.
use crate::{
    automation_batch_notifier::{AutomationNoticeInput, AutomationNoticeTask},
    automation_content_store::text_digest,
    automation_entry_capture::destination,
    fleet_row_ids::FleetRowIds,
    generated_automation_admission as queries,
};
use collaboration_protocol::{AutomationBatch, BatchId, FiringEntry, WorkEntry};
use durable_steps::{AtomicStore, JournalScope, RequestKey, StoreError, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};
use std::collections::BTreeMap;

pub(crate) async fn freeze<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    entries: Vec<FiringEntry>,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
{
    let mut by_machine: BTreeMap<String, Vec<FiringEntry>> = BTreeMap::new();
    for entry in entries {
        by_machine
            .entry(destination(&entry).map_err(StoreError::backend)?)
            .or_default()
            .push(entry);
    }
    for (machine, entries) in by_machine {
        for chunk in entries.chunks(collaboration_protocol::limits::BATCH_MAX_ENTRIES) {
            let batch_id: BatchId = row_ids
                .next()?
                .to_string()
                .try_into()
                .map_err(StoreError::backend)?;
            let batch = AutomationBatch {
                batch_id: batch_id.clone(),
                service_id: machine.clone().try_into().map_err(StoreError::backend)?,
                entries: chunk
                    .iter()
                    .cloned()
                    .map(WorkEntry::Firing)
                    .collect::<Vec<_>>()
                    .try_into()
                    .map_err(StoreError::backend)?,
            };
            let bytes = serde_json::to_string(&batch).map_err(StoreError::backend)?;
            let digest = text_digest(&bytes).map_err(StoreError::backend)?;
            queries::insert_automation_batch(
                scope,
                batch_id.as_str().into(),
                machine.clone(),
                bytes,
                digest.as_str().into(),
            )
            .await
            .map_err(StoreError::backend)?;
            for entry in chunk {
                queries::freeze_automation_firing(
                    scope,
                    batch_id.as_str().into(),
                    machine.clone(),
                    entry.firing_id.as_str().into(),
                )
                .await
                .map_err(StoreError::backend)?;
            }
            let input = AutomationNoticeInput {
                installation: machine.clone().try_into().map_err(StoreError::backend)?,
                batch_id: batch_id.clone(),
            };
            let key = RequestKey::new(format!("automation-batch:{}", batch_id.as_str()))
                .map_err(StoreError::backend)?;
            scope
                .admit::<AutomationNoticeTask<Store, Driver>>(&key, &input)
                .await
                .map_err(StoreError::backend)?;
            scope
                .ensure_wake(SyncTime::from_unix_millis(now_ms))
                .await?;
        }
    }
    Ok(())
}
