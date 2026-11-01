//! Domain preparation precedes writes, so a permanent fault cannot half-admit a Firing.
use crate::{
    automation_catalog_record::{AutomationRecord, CatalogState, StoredAutomationDefinition},
    automation_entry_capture,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, observation_millis},
    fleet_row_ids::FleetRowIds,
    generated_automation_admission as queries,
};
use collaboration_protocol::{FiringEntry, FiringId, GridTime};
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

struct PendingFiring {
    entry: FiringEntry,
    ready: bool,
}
pub(crate) struct PreparedEvaluation {
    record: AutomationRecord,
    entries: Vec<PendingFiring>,
    folds: Vec<(FiringId, GridTime)>,
    changed: bool,
}

pub(crate) async fn prepare<Scope: TransactionScope>(
    scope: &mut Scope,
    mut record: AutomationRecord,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<PreparedEvaluation>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut entries = Vec::new();
    let mut folds = Vec::new();
    let mut changed = false;
    while let Some(due) = record.next_due.filter(|due| due.unix_millis() <= now_ms) {
        changed = true;
        let is_wake = matches!(record.definition, StoredAutomationDefinition::Wake(_));
        let timing = match &record.definition {
            StoredAutomationDefinition::Suspended(_) => {
                return Err(AutomationStoreError::InvalidRecord("suspended.timing"));
            }
            StoredAutomationDefinition::Schedule(schedule) => &schedule.timing,
            StoredAutomationDefinition::Wake(wake) => &wake.timing,
        };
        let next = timing.next_after(due.unix_millis())?;
        if next.is_some_and(|next| next <= due) {
            return Err(AutomationStoreError::InvalidRecord("next_due_ms"));
        }
        if !record
            .progress
            .admitted_through
            .is_some_and(|through| through >= due)
        {
            let fold_into = if is_wake {
                record.progress.occupying.clone()
            } else {
                record.progress.waiting.clone()
            };
            if let Some(firing_id) = fold_into {
                // This changes progress only, never a frozen entry or Batch body.
                folds.push((firing_id, due));
            } else {
                let firing_id: FiringId = row_ids
                    .next()?
                    .to_string()
                    .try_into()
                    .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))?;
                let entry =
                    automation_entry_capture::capture(scope, &record, firing_id.clone(), due)
                        .await?;
                automation_entry_capture::destination(&entry)?;
                let ready =
                    record.progress.occupying.is_none() && record.progress.external.is_none();
                if ready {
                    record.progress.occupying = Some(firing_id);
                } else {
                    record.progress.waiting = Some(firing_id);
                }
                entries.push(PendingFiring { entry, ready });
            }
            record.progress.admitted_through = Some(due);
        }
        let expiry = record
            .expires_at
            .as_ref()
            .map(observation_millis)
            .transpose()?;
        record.next_due = next.filter(|next| expiry.is_none_or(|end| next.unix_millis() < end));
        if is_wake && next.is_none() {
            record.state = CatalogState::Finished;
        }
    }
    Ok(PreparedEvaluation {
        record,
        entries,
        folds,
        changed,
    })
}

pub(crate) async fn commit<Scope: TransactionScope>(
    scope: &mut Scope,
    prepared: PreparedEvaluation,
) -> Result<Vec<FiringEntry>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    if !prepared.changed {
        return Ok(Vec::new());
    }
    let mut ready = Vec::new();
    for pending in prepared.entries {
        let encoded = serde_json::to_string(&pending.entry).map_err(StoreError::backend)?;
        queries::insert_pending_firing(scope, encoded)
            .await
            .map_err(StoreError::backend)?;
        if pending.ready {
            ready.push(pending.entry);
        }
    }
    for (firing_id, due) in prepared.folds {
        queries::fold_automation_firing(
            scope,
            due.unix_millis().to_string(),
            firing_id.as_str().into(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    let record = prepared.record;
    let progress = serde_json::to_string(&serde_json::json!({
        "automation_id": record.id,
        "next_due_ms": record.next_due,
        "admitted_through_ms": record.progress.admitted_through,
        "occupying_firing_id": record.progress.occupying,
        "waiting_firing_id": record.progress.waiting,
        "occupying_external_run": record.progress.external,
    }))
    .map_err(StoreError::backend)?;
    queries::write_automation_progress(scope, progress)
        .await
        .map_err(StoreError::backend)?;
    if record.state == CatalogState::Finished {
        queries::finish_automation_wake(scope, record.id.as_str().into())
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(ready)
}
