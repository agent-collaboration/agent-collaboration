//! A fleet request's first decision is its durable replay authority.

use crate::generated_statements;
#[cfg(feature = "conformance")]
use crate::{
    ObjectHost,
    step_driver::{CommittedEffects, StepCallback, StepDriver, StepResult},
};
#[cfg(feature = "conformance")]
use durable_steps::{AtomicStore, BoxFuture};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetRequestResult;
use message_board::{
    CanonicalHash, DecisionEffect, RecordPlacement, RequestId, RequestLedgerEntry, StoredDecision,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct FleetDecisionStep {
    pub entry: RequestLedgerEntry,
    pub effects: Vec<DecisionEffect>,
    pub receipt_ids: Vec<durable_steps::Uuid>,
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
    write: FleetDecisionStep,
) -> Result<FleetRequestResult, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let request_id = write.entry.request_id.as_str().to_owned();
    let canonical_hash = write.entry.canonical_hash.as_str().to_owned();
    if let Some(stored) = generated_statements::read_fleet_decision(scope, request_id.clone())
        .await
        .map_err(StoreError::backend)?
    {
        let entry = decode(stored)?;
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
    validate_ledger(&write.entry)?;
    let reply = checked_reply(&write.entry, write.effects.clone())?;
    if write.receipt_ids.len() != write.effects.len() {
        return Err(StoreError::Aborted {
            reason: "each Fleet receipt requires one preallocated row ID".into(),
        });
    }
    for id in &write.receipt_ids {
        crate::fleet_row_ids::validate_row_id(*id)?;
    }
    let payload = serde_json::to_string(&write.entry).map_err(StoreError::backend)?;
    let router_time = write
        .entry
        .envelope
        .time
        .as_router_time()
        .ok_or_else(|| corrupt("time_basis", "live decisions require Router time"))?
        .to_rfc3339();
    generated_statements::insert_fleet_decision(
        scope,
        request_id.clone(),
        canonical_hash.clone(),
        payload.clone(),
        router_time.clone(),
    )
    .await
    .map_err(StoreError::backend)?;
    for (ordinal, (effect, receipt_id)) in write.effects.iter().zip(&write.receipt_ids).enumerate()
    {
        let ordinal = i64::try_from(ordinal).map_err(StoreError::backend)?;
        generated_statements::insert_fleet_decision_receipt(
            scope,
            receipt_id.to_string(),
            request_id.clone(),
            ordinal.to_string(),
            serde_json::to_string(effect).map_err(StoreError::backend)?,
        )
        .await
        .map_err(StoreError::backend)?;
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
            generated_statements::insert_fleet_decision(
                scope,
                request_id,
                canonical_hash,
                payload,
                router_time,
            )
            .await
            .map_err(StoreError::backend)?;
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
            let stored = generated_statements::read_fleet_decision(
                scope,
                self.request_id.as_str().to_owned(),
            )
            .await
            .map_err(StoreError::backend)?;
            let entry = stored.map(decode).transpose()?;
            if let Some(entry) = &entry {
                let _reply = checked_reply(entry, read_receipts(scope, &entry.request_id).await?)?;
            }
            Ok((entry, Vec::new()))
        })
    }
}

pub(crate) fn decode(
    stored: generated_statements::StoredFleetDecision,
) -> Result<RequestLedgerEntry, StoreError> {
    let stored_request = RequestId::try_from(required(stored.request_id, "request_id")?)
        .map_err(|error| corrupt("request_id", error))?;
    let stored_hash = CanonicalHash::try_from(required(stored.canonical_hash, "canonical_hash")?)
        .map_err(|error| corrupt("canonical_hash", error))?;
    let payload = required(stored.payload, "payload")?;
    let entry: RequestLedgerEntry =
        serde_json::from_str(&payload).map_err(|error| corrupt("payload", error))?;
    let original: serde_json::Value =
        serde_json::from_str(&payload).map_err(|error| corrupt("payload", error))?;
    if serde_json::to_value(&entry).map_err(StoreError::backend)? != original {
        return Err(corrupt(
            "payload",
            "stored receipt text would be normalized",
        ));
    }
    if entry.request_id != stored_request || entry.canonical_hash != stored_hash {
        return Err(corrupt(
            "payload",
            "receipt identity disagrees with its row",
        ));
    }
    if required(stored.time_basis, "time_basis")? != "router" {
        return Err(corrupt(
            "time_basis",
            "a live request decision needs router time",
        ));
    }
    let router_time: chrono::DateTime<chrono::Utc> = required(stored.router_time, "router_time")?
        .parse()
        .map_err(|error| corrupt("router_time", error))?;
    if Some(router_time) != entry.envelope.time.as_router_time() {
        return Err(corrupt(
            "router_time",
            "receipt time disagrees with its payload",
        ));
    }
    validate_ledger(&entry)?;
    let origin = entry
        .envelope
        .position
        .origin()
        .ok_or_else(|| corrupt("origin", "Fleet decision has no origin"))?;
    if required(stored.workspace_id, "workspace_id")? != entry.envelope.scope.workspace.as_str()
        || stored.board_id.as_deref()
            != entry
                .envelope
                .scope
                .board
                .as_ref()
                .map(|board| board.as_str())
        || required(stored.origin_installation, "origin_installation")?
            != origin.installation.as_str()
        || required(stored.origin_machine, "origin_machine")? != origin.machine.as_str()
    {
        return Err(corrupt(
            "origin or scope",
            "decision headers disagree with their payload",
        ));
    }
    Ok(entry)
}

pub(crate) fn validate_ledger(entry: &RequestLedgerEntry) -> Result<(), StoreError> {
    message_board::Record::Request(entry.clone())
        .validate_bounds()
        .map_err(|error| corrupt("payload", error))?;
    let envelope = &entry.envelope;
    if envelope.id.as_str() != entry.request_id.as_str()
        || !matches!(envelope.position, RecordPlacement::Fleet(_))
        || envelope.scope.project.is_some()
        || entry.accepted_in.is_some()
    {
        return Err(corrupt(
            "envelope",
            "request ledger violates Fleet record invariants",
        ));
    }
    Ok(())
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
        .map(|(expected, row)| {
            let id = required(row.receipt_id, "receipt_id")?;
            let parsed = durable_steps::Uuid::parse_str(&id)
                .map_err(|error| corrupt("receipt_id", error))?;
            let ordinal = required(row.ordinal, "ordinal")?;
            if parsed.get_version_num() != 7
                || parsed.to_string() != id
                || required(row.ordinal_class, "ordinal_class")? != "integer"
                || ordinal != expected.to_string()
            {
                return Err(corrupt("receipt", "noncanonical identity or ordinal"));
            }
            let payload = required(row.effect, "effect")?;
            let effect: DecisionEffect =
                serde_json::from_str(&payload).map_err(|error| corrupt("effect", error))?;
            message_board::validate_fleet_decision_effect(&effect)
                .map_err(|error| corrupt("effect", error))?;
            if serde_json::to_string(&effect).map_err(StoreError::backend)? != payload {
                return Err(corrupt("effect", "noncanonical effect"));
            }
            Ok(effect)
        })
        .collect()
}

fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field, "required receipt column is NULL"))
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("fleet_request_decisions.{field}"),
        message: message.to_string(),
    }
}

#[cfg(feature = "conformance")]
impl<Host: ObjectHost> StepDriver<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: sqlx_core::arguments::IntoArguments<Host::Database>,
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
    ) -> Result<CommittedEffects<fleet_admission::FleetRequestResult>, StoreError> {
        let (_, effects) = self
            .run(crate::fleet_decision_step::FleetDecisionStep {
                entry,
                effects,
                receipt_ids,
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
    ) -> Result<CommittedEffects<fleet_admission::FleetRequestResult>, StoreError> {
        let (_, effects) = self
            .run(crate::fleet_decision_step::FleetDecisionStep {
                entry,
                effects,
                receipt_ids,
                fault,
            })
            .await?;
        Ok(effects)
    }
}
