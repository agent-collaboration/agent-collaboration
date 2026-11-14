//! A fleet request's first decision is its durable replay authority.

use crate::generated_statements;
#[cfg(feature = "conformance")]
use crate::{
    LegacyObjectHost,
    step_driver::{CommittedEffects, StepCallback, StepDriver, StepResult},
};
#[cfg(feature = "conformance")]
use durable_steps::{AtomicStore, BoxFuture};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetRequestResult;
use message_board::{DecisionEffect, RequestId, RequestLedgerEntry, StoredDecision};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct FleetDecisionStep {
    pub entry: RequestLedgerEntry,
    pub effects: Vec<DecisionEffect>,
    pub receipt_ids: Vec<durable_steps::Uuid>,
    pub now: runtime_primitives::SyncTime,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
    #[cfg(feature = "conformance")]
    pub fault: FleetDecisionFault,
}

#[cfg(feature = "conformance")]
pub(crate) struct ReadFleetDecisionStep {
    pub request_id: RequestId,
}

#[cfg(feature = "conformance")]
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FleetDecisionFault {
    #[default]
    None,
    CallbackAfterWrite,
    SqlAfterWrite,
}

#[cfg(feature = "conformance")]
impl<Store, Driver> StepCallback<Store, Driver> for FleetDecisionStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = FleetRequestResult;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let answer = persist_decision(scope, self).await?;
            Ok(((), vec![answer]))
        })
    }
}

pub(crate) async fn persist_decision<Scope: TransactionScope>(
    scope: &mut Scope,
    mut write: FleetDecisionStep,
) -> Result<FleetRequestResult, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if let Some(entry) = read_ledger(scope, &write.entry.request_id).await? {
        let answer = if entry.canonical_hash == write.entry.canonical_hash {
            FleetRequestResult::Decided(checked_reply(
                &entry,
                read_receipts(scope, &entry.request_id).await?,
            )?)
        } else {
            FleetRequestResult::ConflictingRequest
        };
        return Ok(answer);
    }
    crate::fleet_record_codec::validate_ledger(&write.entry)?;
    let reply = checked_reply(&write.entry, write.effects.clone())?;
    if write.receipt_ids.len() != write.effects.len() {
        return Err(StoreError::Aborted {
            reason: "each Fleet receipt requires one preallocated row ID".into(),
        });
    }
    for id in &write.receipt_ids {
        crate::fleet_row_ids::validate_row_id(*id)?;
    }
    let nodes = match &write.entry.decision {
        StoredDecision::Rejected { reasons } => {
            crate::fleet_reason_tree::encode(reasons, &mut write.row_ids)?
        }
        StoredDecision::Accepted { .. } => Vec::new(),
    };
    crate::fleet_ledger_codec::insert(scope, &write.entry, write.now).await?;
    crate::fleet_reason_write::insert(
        scope,
        &write.entry.request_id,
        nodes,
        write.now,
        &mut write.row_ids,
    )
    .await?;
    for (ordinal, (effect, receipt_id)) in write.effects.iter().zip(&write.receipt_ids).enumerate()
    {
        crate::fleet_receipt_codec::insert(
            scope,
            &write.entry.request_id,
            *receipt_id,
            ordinal,
            effect,
            write.now,
        )
        .await?;
    }

    #[cfg(feature = "conformance")]
    match write.fault {
        FleetDecisionFault::None => {}
        FleetDecisionFault::CallbackAfterWrite => {
            return Err(StoreError::Aborted {
                reason: "conformance callback failure after the receipt write".into(),
            });
        }
        FleetDecisionFault::SqlAfterWrite => {
            // The real primary-key violation must roll the first write back.
            crate::fleet_ledger_codec::insert(scope, &write.entry, write.now).await?;
        }
    }
    Ok(FleetRequestResult::Decided(reply))
}

#[cfg(feature = "conformance")]
impl<Store, Driver> StepCallback<Store, Driver> for ReadFleetDecisionStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<RequestLedgerEntry>;
    type Effect = FleetRequestResult;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let entry = read_ledger(scope, &self.request_id).await?;
            if let Some(entry) = &entry {
                let _reply = checked_reply(entry, read_receipts(scope, &entry.request_id).await?)?;
            }
            Ok((entry, Vec::new()))
        })
    }
}

/// Read a caller's immutable request authorities together in its existing scope.
pub(crate) async fn read_ledgers<S: TransactionScope>(
    scope: &mut S,
    requests: &[RequestId],
) -> Result<std::collections::BTreeMap<RequestId, RequestLedgerEntry>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let expected: std::collections::BTreeSet<_> = requests.iter().cloned().collect();
    let mut entries = std::collections::BTreeMap::new();
    if expected.is_empty() {
        return Ok(entries);
    }
    // JSON is only the bound set of typed IDs, never a stored record column.
    let ids = expected.iter().map(RequestId::as_str).collect::<Vec<_>>();
    let rows = generated_statements::read_fleet_decisions(
        scope,
        serde_json::to_string(&ids).map_err(StoreError::backend)?,
    )
    .await
    .map_err(StoreError::backend)?;
    let headers = rows
        .into_iter()
        .map(crate::fleet_ledger_codec::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let rejected = headers
        .iter()
        .filter(|header| header.rejected())
        .map(|header| header.request.clone())
        .collect::<Vec<_>>();
    let mut trees = crate::fleet_reason_tree::ReasonTrees::read(scope, &rejected).await?;
    for header in headers {
        let entry = header.record(&mut trees)?;
        if !expected.contains(&entry.request_id)
            || entries.insert(entry.request_id.clone(), entry).is_some()
        {
            return Err(corrupt(
                "request_id",
                "batch returned duplicate or unrequested authority",
            ));
        }
    }
    trees.finish()?;
    Ok(entries)
}

pub(crate) async fn read_ledger<S: TransactionScope>(
    scope: &mut S,
    request: &RequestId,
) -> Result<Option<RequestLedgerEntry>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    Ok(read_ledgers(scope, std::slice::from_ref(request))
        .await?
        .remove(request))
}

pub(crate) fn checked_reply(
    entry: &RequestLedgerEntry,
    effects: Vec<DecisionEffect>,
) -> Result<message_board::RequestDecision, StoreError> {
    for effect in &effects {
        message_board::validate_fleet_decision_effect(effect)
            .map_err(|error| corrupt("effects", error))?;
    }
    if matches!(entry.decision, StoredDecision::Rejected { .. }) && !effects.is_empty() {
        return Err(corrupt(
            "effects",
            "receipt must belong to a Fleet decision",
        ));
    }
    entry
        .decision
        .clone()
        .into_reply(effects, None)
        .map_err(|error| corrupt("effects", error))
}

pub(crate) async fn read_receipts<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &RequestId,
) -> Result<Vec<DecisionEffect>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let rows = generated_statements::read_fleet_decision_receipts(scope, request.as_str().into())
        .await
        .map_err(StoreError::backend)?;
    rows.into_iter()
        .enumerate()
        .map(|(ordinal, row)| {
            crate::fleet_receipt_codec::decode(
                row,
                request,
                u64::try_from(ordinal).map_err(StoreError::backend)?,
            )
        })
        .collect()
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("fleet_request_decisions.{field}"),
        message: message.to_string(),
    }
}

#[cfg(feature = "conformance")]
impl<Host: LegacyObjectHost> StepDriver<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: sqlx_core::arguments::IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        sqlx_core::types::Type<Host::Database> + sqlx_core::encode::Encode<'value, Host::Database>,
    for<'value> Option<String>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::decode::Decode<'value, Host::Database>
        + sqlx_core::encode::Encode<'value, Host::Database>,
    for<'row> &'row str: sqlx_core::column::ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn record_fleet_decision(
        &self,
        entry: message_board::RequestLedgerEntry,
        effects: Vec<DecisionEffect>,
        receipt_ids: Vec<durable_steps::Uuid>,
        now: runtime_primitives::SyncTime,
        ids: std::sync::Arc<dyn runtime_primitives::IdSource>,
    ) -> Result<CommittedEffects<fleet_admission::FleetRequestResult>, StoreError> {
        let (_, effects) = self
            .run(crate::fleet_decision_step::FleetDecisionStep {
                entry,
                effects,
                receipt_ids,
                now,
                row_ids: crate::fleet_row_ids::FleetRowIds::new(ids),
                #[cfg(feature = "conformance")]
                fault: crate::fleet_decision_step::FleetDecisionFault::None,
            })
            .await?;
        Ok(effects)
    }

    pub(crate) async fn read_fleet_decision(
        &self,
        request_id: message_board::RequestId,
    ) -> Result<Option<message_board::RequestLedgerEntry>, StoreError> {
        let (entry, _) = self
            .run(crate::fleet_decision_step::ReadFleetDecisionStep { request_id })
            .await?;
        Ok(entry)
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn conformance_record_fleet_decision(
        &self,
        entry: message_board::RequestLedgerEntry,
        effects: Vec<DecisionEffect>,
        receipt_ids: Vec<durable_steps::Uuid>,
        fault: crate::fleet_decision_step::FleetDecisionFault,
        now: runtime_primitives::SyncTime,
        ids: std::sync::Arc<dyn runtime_primitives::IdSource>,
    ) -> Result<CommittedEffects<fleet_admission::FleetRequestResult>, StoreError> {
        let (_, effects) = self
            .run(crate::fleet_decision_step::FleetDecisionStep {
                entry,
                effects,
                receipt_ids,
                now,
                row_ids: crate::fleet_row_ids::FleetRowIds::new(ids),
                fault,
            })
            .await?;
        Ok(effects)
    }
}
