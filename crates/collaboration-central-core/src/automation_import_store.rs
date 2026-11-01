//! One Fleet transaction admits frozen handover bytes and retained local work.
use crate::{
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, StoredAutomationDefinition,
        StoredSuspendedConfiguration,
    },
    automation_catalog_store as catalog, automation_import_content as content,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required, timestamp},
    fleet_row_ids::FleetRowIds,
    generated_automation_admission as admissions, generated_automation_catalog as queries,
};
use collaboration_mail_protocol::MailDigest;
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationImportIdentity, AutomationImportRequest,
    AutomationRefusal, ContentReference, FiringEntry, FiringId, FiringWork, ImportId,
    ScheduleDefinition,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sha2::{Digest, Sha256};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn import<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    request: &AutomationImportRequest,
    installation: &ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let payload = request.payload.payload();
    let digest = receipt_digest(installation, request)?;
    let receipts = queries::read_automation_imports(
        scope,
        request.import_id.as_str().into(),
        payload.automation_id.as_str().into(),
    )
    .await?;
    let mut replay = false;
    for receipt in receipts {
        let previous_id: ImportId = required(receipt.import_id, "automation_imports.import_id")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("automation_imports.import_id"))?;
        let previous_service: ServiceId =
            required(receipt.service_id, "automation_imports.service_id")?
                .try_into()
                .map_err(|_| {
                    AutomationStoreError::InvalidRecord("automation_imports.service_id")
                })?;
        let previous_automation: AutomationId =
            required(receipt.automation_id, "automation_imports.automation_id")?
                .try_into()
                .map_err(|_| {
                    AutomationStoreError::InvalidRecord("automation_imports.automation_id")
                })?;
        let previous_digest: MailDigest = required(receipt.digest, "automation_imports.digest")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("automation_imports.digest"))?;
        if previous_automation != payload.automation_id
            || previous_service != *installation
            || previous_digest != digest
        {
            return Err(content::conflict(payload));
        }
        // A replay may use a new import ID, but a reused ID may never name other work.
        replay |= previous_id == request.import_id || previous_automation == payload.automation_id;
    }
    let identity = AutomationImportIdentity {
        automation_id: payload.automation_id.clone(),
    };
    if replay {
        return Ok(AutomationAnswer::AlreadyImported(identity));
    }
    if queries::read_automation(scope, payload.automation_id.as_str().into())
        .await?
        .is_some()
    {
        return Err(content::conflict(payload));
    }
    timestamp(now_ms)?;
    let prepared_content = content::prepare(scope, payload, row_ids).await?;
    let mut record = crate::automation_import_record::prepare(
        &request.payload,
        installation,
        &prepared_content.reference,
        now_ms,
    )?;
    require_free_slot(scope, &record).await?;
    if let Some(carried) = &payload.carried_firing {
        if crate::generated_automation_firing::read_automation_firing(
            scope,
            carried.firing_id.as_str().into(),
        )
        .await?
        .is_some()
        {
            return Err(content::conflict(payload));
        }
        record.progress.occupying = Some(carried.firing_id.clone());
    }
    let waiter = payload
        .carried_waiting_due_ms
        .map(|due| {
            let id: FiringId = row_ids
                .next()?
                .to_string()
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))?;
            carried_waiter(&record, &prepared_content.reference, id, due)
        })
        .transpose()?;
    if let Some(waiter) = &waiter {
        if record.progress.external.is_some() {
            record.progress.waiting = Some(waiter.firing_id.clone());
        } else {
            record.progress.occupying = Some(waiter.firing_id.clone());
        }
    }
    let waiter_json = waiter
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| AutomationStoreError::InvalidRecord("pending_entry_json"))?;
    let carried_json = payload.carried_firing.as_ref().map(|carried| serde_json::to_string(&serde_json::json!({
        "firingId":carried.firing_id,"automationId":record.id,"dueMs":carried.due_ms,
        "revision":record.revision,"contentId":prepared_content.reference.content_id,"serviceId":installation
    }))).transpose().map_err(|_| AutomationStoreError::InvalidRecord("carried_firing"))?;
    let progress_json = serde_json::to_string(&serde_json::json!({
        "automationId":record.id,"watermarkLocalMs":payload.watermark_local_ms,"admittedThroughMs":record.progress.admitted_through,
        "occupyingFiringId":record.progress.occupying,"waitingFiringId":record.progress.waiting,"externalOccupant":record.progress.external
    })).map_err(|_| AutomationStoreError::InvalidRecord("import.progress"))?;
    // All expected refusals and executable inputs above are resolved before the first write.
    let committed: AutomationStoreResult<()> = async {
        content::commit(scope, &prepared_content, payload, now_ms).await?;
        catalog::insert(scope, &record).await?;
        if let Some(waiter) = waiter_json {
            admissions::insert_pending_firing(scope, waiter).await?;
        }
        if let Some(carried) = carried_json {
            queries::insert_carried_automation_firing(scope, carried).await?;
        }
        queries::write_automation_import_progress(scope, progress_json).await?;
        queries::insert_automation_import(
            scope,
            request.import_id.as_str().into(),
            installation.as_str().into(),
            record.id.as_str().into(),
            digest.as_str().into(),
        )
        .await?;
        let ready = waiter.filter(|entry| {
            record.progress.external.is_none() && entry.due_at.unix_millis() <= now_ms
        });
        crate::automation_batch_freeze::freeze::<Store, Driver>(
            scope,
            ready.into_iter().collect(),
            now_ms,
            row_ids,
        )
        .await?;
        Ok(())
    }
    .await;
    committed.map_err(|error| AutomationStoreError::Storage(StoreError::backend(error)))?;
    Ok(AutomationAnswer::Imported(identity))
}

fn receipt_digest(
    installation: &ServiceId,
    request: &AutomationImportRequest,
) -> AutomationStoreResult<MailDigest> {
    let mut digest = Sha256::new();
    digest.update(b"automationImport\0");
    digest.update(installation.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(request.payload.as_bytes());
    format!("{:x}", digest.finalize())
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_imports.digest"))
}

async fn require_free_slot<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if !record.state.owns_wake_slot() {
        return Ok(());
    }
    let target = match &record.definition {
        StoredAutomationDefinition::Wake(wake) => Some(&wake.target),
        StoredAutomationDefinition::Suspended(definition) => match &definition.configuration {
            StoredSuspendedConfiguration::Wake(wake) => Some(&wake.target),
            StoredSuspendedConfiguration::Schedule(_) => None,
        },
        StoredAutomationDefinition::Schedule(_) => None,
    };
    let Some(target) = target else {
        return Ok(());
    };
    let Some(previous) =
        queries::find_wake_slot(scope, crate::automation_catalog_record::wake_slot(target)?)
            .await?
    else {
        return Ok(());
    };
    let raw: serde_json::Value =
        serde_json::from_str(&required(previous.creator_json, "creator_json")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    crate::automation_request_decode::identity_limits(&raw)?;
    let creator: BoundAutomationCreator = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    Err(AutomationStoreError::Refused(
        AutomationRefusal::WakeExists {
            creator: creator.declared,
        },
    ))
}

fn carried_waiter(
    record: &AutomationRecord,
    content: &ContentReference,
    firing_id: FiringId,
    due: collaboration_protocol::GridTime,
) -> AutomationStoreResult<FiringEntry> {
    let definition = match &record.definition {
        StoredAutomationDefinition::Schedule(schedule) => schedule.definition.clone(),
        StoredAutomationDefinition::Suspended(definition) => {
            let StoredSuspendedConfiguration::Schedule(configuration) = &definition.configuration
            else {
                return Err(AutomationStoreError::InvalidRecord(
                    "carried_waiting_due_ms",
                ));
            };
            ScheduleDefinition {
                instruction_id: configuration.instruction_id.clone(),
                timing: crate::automation_import_record::one_shot(due)?,
                enabled: configuration.enabled,
                destination: configuration.destination.clone(),
                execution_timeout_seconds: configuration.execution_timeout_seconds,
                model: configuration.model.clone(),
                effort: configuration.effort.clone(),
            }
        }
        StoredAutomationDefinition::Wake(_) => {
            return Err(AutomationStoreError::InvalidRecord(
                "carried_waiting_due_ms",
            ));
        }
    };
    let entry = FiringEntry {
        firing_id,
        automation_id: record.id.clone(),
        due_at: due,
        revision: record.revision.clone(),
        content: content.clone(),
        work: FiringWork::Schedule { definition },
    };
    crate::automation_entry_capture::destination(&entry)?;
    Ok(entry)
}
