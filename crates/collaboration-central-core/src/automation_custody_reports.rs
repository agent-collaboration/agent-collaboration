//! Monotone reports retire duplicate outbox items and release waiting work atomically.
use crate::{
    automation_firing_store,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    automation_waiting_release as waiting,
    fleet_row_ids::FleetRowIds,
    generated_automation_custody as queries,
};
use collaboration_protocol::{
    AcceptedWorkReport, AutomationAnswer, AutomationRefusal, FiringSnapshot, ReportWorkItems,
    WorkItemReport, WorkReportAcknowledgements, WorkReportSubject,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn report<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    installation: &ServiceId,
    request: &ReportWorkItems,
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
    // Validate every kind in the complete envelope before the first write.
    for item in request.reports() {
        match item {
            WorkItemReport::Firing(report) => {
                validate_firing_report(scope, installation, report).await?
            }
            WorkItemReport::SessionCreate {
                work_item_id,
                create,
                ..
            } => {
                crate::machine_work_item_reports::validate(
                    scope,
                    installation,
                    work_item_id,
                    create,
                )
                .await?;
            }
        }
    }
    let mut accepted = Vec::new();
    let mut ready = Vec::new();
    for item in request.reports() {
        let (subject, seq) = match item {
            WorkItemReport::Firing(report) => {
                if let Some(entry) = apply_firing_report(scope, report).await? {
                    ready.push(entry);
                }
                (
                    WorkReportSubject::Firing {
                        firing_id: report.firing_id.clone(),
                    },
                    report.seq,
                )
            }
            WorkItemReport::SessionCreate {
                work_item_id,
                seq,
                create,
                input,
            } => {
                crate::machine_work_item_reports::apply(scope, work_item_id, *seq, create, input)
                    .await?;
                (
                    WorkReportSubject::SessionCreate {
                        work_item_id: work_item_id.clone(),
                    },
                    *seq,
                )
            }
        };
        accepted.push(AcceptedWorkReport { subject, seq });
    }
    ready.retain(|entry| entry.due_at.unix_millis() <= now_ms);
    crate::automation_batch_freeze::freeze::<Store, Driver>(scope, ready, now_ms, row_ids).await?;
    Ok(AutomationAnswer::Reports(WorkReportAcknowledgements {
        accepted,
    }))
}

async fn validate_firing_report<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    report: &collaboration_protocol::FiringReport,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let owner = queries::read_firing_owner(scope, report.firing_id.as_str().into())
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
    let service: ServiceId = owner
        .service_id
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("report_service_id"))?;
    if service != *installation {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::OriginMismatch,
        ));
    }
    automation_firing_store::read(scope, &report.firing_id)
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
    Ok(())
}

async fn apply_firing_report<Scope: TransactionScope>(
    scope: &mut Scope,
    report: &collaboration_protocol::FiringReport,
) -> AutomationStoreResult<Option<collaboration_protocol::FiringEntry>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    // Re-read inside the transaction: one envelope may contain reordered
    // reports for the same Firing, and each sees the preceding accepted seq.
    let current = automation_firing_store::read(scope, &report.firing_id)
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| StoreError::backend("owned Firing disappeared"))?;
    if u64::from(report.seq) > current.report_seq.map(u64::from).unwrap_or(0) {
        let outcome = if current.outcome.is_terminal() {
            &current.outcome
        } else {
            &report.outcome
        };
        let raw = serde_json::to_value(outcome).map_err(StoreError::backend)?;
        let state = raw
            .get("state")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StoreError::backend("Firing outcome lacks state"))?
            .to_owned();
        queries::write_firing_report(
            scope,
            u64::from(report.seq).to_string(),
            state,
            serde_json::to_string(outcome).map_err(StoreError::backend)?,
            report.firing_id.as_str().into(),
        )
        .await
        .map_err(StoreError::backend)?;
        if !current.outcome.is_terminal()
            && outcome.is_terminal()
            && let Some(entry) = release(scope, &current)
                .await
                .map_err(StoreError::backend)?
        {
            return Ok(Some(entry));
        }
    }

    Ok(None)
}

async fn release<Scope: TransactionScope>(
    scope: &mut Scope,
    current: &FiringSnapshot,
) -> AutomationStoreResult<Option<collaboration_protocol::FiringEntry>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let progress = queries::read_custody_progress(scope, current.automation_id.as_str().into())
        .await?
        .ok_or(AutomationStoreError::InvalidRecord("automation_timing"))?;
    if progress.occupying_firing_id.as_deref() != Some(current.firing_id.as_str()) {
        return Ok(None);
    }
    let ready = waiting::prepare(scope, progress, &current.automation_id).await?;
    waiting::commit(scope, &current.automation_id, ready.as_ref()).await?;
    Ok(ready)
}

pub(crate) async fn settle_external<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    installation: &ServiceId,
    request: &collaboration_protocol::LocalRunSettled,
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
    let owner = queries::read_automation_import_owner(
        scope,
        request.automation_id.as_str().into(),
        installation.as_str().into(),
    )
    .await?
    .ok_or(AutomationStoreError::Refused(
        AutomationRefusal::OriginMismatch,
    ))?;
    crate::automation_store_error::row_id(&required(owner.import_id, "import_id")?, "import_id")?;
    let progress = queries::read_custody_progress(scope, request.automation_id.as_str().into())
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
    match progress.occupying_external_run.as_deref() {
        None => return Ok(AutomationAnswer::Acknowledged),
        Some(run) if run != request.run_id.as_str() => {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::OriginMismatch,
            ));
        }
        Some(run) => {
            crate::automation_store_error::row_id(run, "occupying_external_run")?;
        }
    }
    let ready = waiting::prepare(scope, progress, &request.automation_id).await?;
    waiting::commit(scope, &request.automation_id, ready.as_ref()).await?;
    let ready = ready.filter(|entry| entry.due_at.unix_millis() <= now_ms);
    crate::automation_batch_freeze::freeze::<Store, Driver>(
        scope,
        ready.into_iter().collect(),
        now_ms,
        row_ids,
    )
    .await?;
    Ok(AutomationAnswer::Acknowledged)
}
