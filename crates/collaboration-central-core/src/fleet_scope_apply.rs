//! Apply a shared Fleet decision once, in the transaction that read its inputs.

use crate::{
    fleet_decision_step::{FleetDecisionStep, persist_decision},
    fleet_record_codec::{FleetRecordHeader, corrupt},
    fleet_record_store::{self, RecordWriteSource},
};
use chrono::{DateTime, Utc};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{FleetDecision, FleetImportDecision, FleetRequestResult, FleetWrite};
use message_board::{DecisionEffect, Record, RequestLedgerEntry};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn import<Scope: TransactionScope>(
    scope: &mut Scope,
    decision: &FleetImportDecision,
    now: DateTime<Utc>,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    for write in &decision.writes {
        fleet_record_store::put(
            scope,
            &write.record,
            RecordWriteSource::Imported(&write.digest),
            row_ids,
        )
        .await?;
    }
    crate::fleet_feed_store::append(scope, &decision.feed, now, row_ids).await
}

pub(crate) async fn decision<Scope: TransactionScope>(
    scope: &mut Scope,
    decision: FleetDecision,
    now: DateTime<Utc>,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<FleetRequestResult, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if decision.writes.is_empty() {
        if !decision.feed.is_empty() {
            return Err(corrupt("decision", "replay cannot append feed entries"));
        }
        return Ok(FleetRequestResult::Decided(decision.decision));
    }
    let mut ledger: Option<RequestLedgerEntry> = None;
    for write in &decision.writes {
        if let FleetWrite::Ledger(entry) = write {
            if ledger.is_some() {
                return Err(corrupt("decision", "multiple ledgers in one decision"));
            }
            ledger = Some(entry.as_ref().clone());
        }
    }
    let ledger = ledger.ok_or_else(|| corrupt("decision", "new decision has no ledger"))?;
    let mut effects: Vec<DecisionEffect> = Vec::new();
    let mut creation = None;
    for write in decision.writes {
        match write {
            FleetWrite::BindCreation {
                project,
                creating_request,
                carrier_installation,
            } => {
                if creating_request != ledger.request_id
                    || ledger
                        .envelope
                        .position
                        .origin()
                        .is_none_or(|origin| origin.installation != carrier_installation)
                    || creation.is_some()
                {
                    return Err(corrupt(
                        "creation_decisions",
                        "binding does not belong to this ledger's carrier",
                    ));
                }
                creation = Some((project, creating_request, carrier_installation));
            }
            FleetWrite::Put(record) => {
                fleet_record_store::put(scope, &record, RecordWriteSource::Live, row_ids).await?
            }
            FleetWrite::Ledger(_) => {}
            FleetWrite::DecisionReceipt {
                request,
                ordinal,
                effect,
            } => {
                if request != ledger.request_id
                    || u64::try_from(effects.len()).map_err(StoreError::backend)? != ordinal
                {
                    return Err(corrupt(
                        "decision.receipts",
                        "receipt has another request or noncontiguous ordinal",
                    ));
                }
                effects.push(effect);
            }
            FleetWrite::RepositoryAttach {
                workspace,
                repository,
            } => {
                // The shared decision's workspace Put owns both the repository list
                // and this association. Use its accepting time and origin.
                let stored = fleet_record_store::read(scope, &workspace.clone().into())
                    .await?
                    .ok_or_else(|| corrupt("repository", "workspace is missing"))?;
                let Record::Workspace(value) = &stored.record else {
                    return Err(corrupt("repository", "wrong parent kind"));
                };
                if !value.repositories.contains(&repository) {
                    return Err(corrupt("repository", "attachment disagrees with workspace"));
                }
                let header = FleetRecordHeader::for_record(&stored.record)?;
                crate::fleet_repository_store::attach(
                    scope,
                    &workspace,
                    &repository,
                    &header,
                    None,
                    row_ids.next()?,
                )
                .await?;
            }
            FleetWrite::RepositoryDetach {
                workspace,
                repository,
            } => crate::fleet_repository_store::detach(scope, &workspace, &repository).await?,
        }
    }
    let receipt_ids = (0..effects.len())
        .map(|_| row_ids.next())
        .collect::<Result<Vec<_>, _>>()?;
    let result = persist_decision(
        scope,
        FleetDecisionStep {
            entry: ledger,
            effects,
            receipt_ids,
            #[cfg(feature = "conformance")]
            fault: crate::fleet_decision_step::FleetDecisionFault::None,
        },
    )
    .await?;
    if result != FleetRequestResult::Decided(decision.decision) {
        return Err(corrupt(
            "decision",
            "saved decision disagrees with shared rules",
        ));
    }
    if let Some((project, request, carrier)) = creation {
        crate::fleet_creation_store::insert(scope, &project, &request, &carrier).await?;
    }
    crate::fleet_feed_store::append(scope, &decision.feed, now, row_ids).await?;
    Ok(result)
}
