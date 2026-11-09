//! Where a claim's settlement is recorded as owed, and how far it has got.
//!
//! A claim that ends without settling its reservation (refused, cancelled or
//! failed) still owes the reservation a settlement. The lease rules call
//! [`OwedSettlement::owe_settlement`] in the transaction that ends the claim,
//! and the holder-change view asks [`SettlementStateSource::settlement_state`]
//! how far it has got. The host type a registry is composed with chooses the
//! owner: [`NoLeaseChangeHook`](crate::NoLeaseChangeHook) admits
//! [`SettleReservationTask`](crate::SettleReservationTask) and reads its journal
//! row ([`journaled_owe_settlement`], [`journaled_settlement_state`]); central
//! supplies its own.

use crate::Waiting;
use crate::{LeaseError, registry_storage::RegistryStorage};
use durable_steps::{
    AtomicStore, JournalScope, RequestKey,
    storage::{JournalStorage, TransactionScope},
};
use fleet_protocol::{ClaimSettlement, FailureCode, SyncTime, TransferId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::future::Future;

/// How far an owed settlement has got.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementState {
    /// Still owed; `waiting` names what it waits on, when known.
    Owed { waiting: Option<Waiting> },
    /// The reservation is settled.
    Settled,
    /// Settling failed for good.
    Failed { reason: FailureCode },
}

/// Records a claim's settlement as owed, in the transaction that ends the claim.
pub trait OwedSettlement<S, D>: Send + Sync + 'static
where
    S: AtomicStore,
    D: Database,
    for<'t> S::Scope<'t>: TransactionScope<Database = D>,
    D::Arguments: IntoArguments<D>,
    for<'value> String: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'value> Option<String>: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'row> &'row str: ColumnIndex<D::Row>,
{
    fn owe_settlement<'call, 'scope: 'call>(
        scope: &'call mut S::Scope<'scope>,
        transfer: TransferId,
        settlement: ClaimSettlement,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), LeaseError>> + Send + 'call;
}

/// Answers how far a claim's owed settlement has got: `None` when none was
/// recorded as owed.
pub trait SettlementStateSource: Send + Sync + 'static {
    fn settlement_state<'call, Scope>(
        scope: &'call mut Scope,
        transfer: TransferId,
    ) -> impl Future<Output = Result<Option<SettlementState>, LeaseError>> + Send + 'call
    where
        Scope: TransactionScope + JournalStorage,
        <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
        for<'value> String: Type<Scope::Database>
            + Encode<'value, Scope::Database>
            + Decode<'value, Scope::Database>,
        for<'value> Option<String>: Type<Scope::Database>
            + Encode<'value, Scope::Database>
            + Decode<'value, Scope::Database>,
        for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>;
}

/// The settlement's journal key: one settlement per claim attempt.
pub(crate) fn settlement_key(transfer: TransferId) -> Result<RequestKey, LeaseError> {
    RequestKey::new(format!("claim-settle:{}", transfer.as_uuid())).map_err(|error| {
        LeaseError::InvalidInput {
            field: error.to_string(),
        }
    })
}

/// Owe the settlement by admitting `SettleReservationTask`, recording its
/// operation as the admission's follow-up (today's journaled owner).
pub async fn journaled_owe_settlement<S, D>(
    scope: &mut S::Scope<'_>,
    transfer: TransferId,
    settlement: ClaimSettlement,
    now: SyncTime,
) -> Result<(), LeaseError>
where
    S: AtomicStore,
    D: Database,
    for<'t> S::Scope<'t>: TransactionScope<Database = D>,
    D::Arguments: IntoArguments<D>,
    for<'value> String: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'value> Option<String>: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'row> &'row str: ColumnIndex<D::Row>,
{
    let mut admission = RegistryStorage(&mut *scope)
        .admission(transfer)
        .await?
        .ok_or_else(|| LeaseError::Corrupt {
            field: "settlement admission".into(),
        })?;
    let admitted = scope
        .admit::<crate::SettleReservationTask<S>>(&settlement_key(transfer)?, &settlement)
        .await
        .map_err(|error| LeaseError::InvalidInput {
            field: error.to_string(),
        })?;
    admission.follow_up = Some(admitted.id());
    RegistryStorage(scope)
        .save_admission(&admission, now)
        .await?;
    Ok(())
}

/// The journaled settlement's state, read from the admission's follow-up
/// operation.
pub async fn journaled_settlement_state<Scope>(
    scope: &mut Scope,
    transfer: TransferId,
) -> Result<Option<SettlementState>, LeaseError>
where
    Scope: TransactionScope + JournalStorage,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(admission) = RegistryStorage(&mut *scope).admission(transfer).await? else {
        return Ok(None);
    };
    let Some(operation) = admission.follow_up else {
        return Ok(None);
    };
    crate::holder_change_view::journal_follow_up(
        scope,
        operation,
        "fleet-sync.settle-reservation",
        &settlement_key(transfer)?,
    )
    .await
    .map(Some)
}

impl<S, D> OwedSettlement<S, D> for crate::NoLeaseChangeHook
where
    S: AtomicStore,
    D: Database,
    for<'t> S::Scope<'t>: TransactionScope<Database = D>,
    D::Arguments: IntoArguments<D>,
    for<'value> String: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'value> Option<String>: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'row> &'row str: ColumnIndex<D::Row>,
{
    fn owe_settlement<'call, 'scope: 'call>(
        scope: &'call mut S::Scope<'scope>,
        transfer: TransferId,
        settlement: ClaimSettlement,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), LeaseError>> + Send + 'call {
        journaled_owe_settlement::<S, D>(scope, transfer, settlement, now)
    }
}

impl SettlementStateSource for crate::NoLeaseChangeHook {
    fn settlement_state<'call, Scope>(
        scope: &'call mut Scope,
        transfer: TransferId,
    ) -> impl Future<Output = Result<Option<SettlementState>, LeaseError>> + Send + 'call
    where
        Scope: TransactionScope + JournalStorage,
        <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
        for<'value> String: Type<Scope::Database>
            + Encode<'value, Scope::Database>
            + Decode<'value, Scope::Database>,
        for<'value> Option<String>: Type<Scope::Database>
            + Encode<'value, Scope::Database>
            + Decode<'value, Scope::Database>,
        for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
    {
        journaled_settlement_state(scope, transfer)
    }
}
