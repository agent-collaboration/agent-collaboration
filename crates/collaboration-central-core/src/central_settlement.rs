//! Central's owner of claim settlements.
//!
//! A claim that ends without settling its reservation owes it a settlement
//! (see `fleet_project_lease::OwedSettlement`). Fleet's lease rules and the
//! holder-change view reach central's owner through [`CentralSettlement`]: the
//! lease change hook delegates to it, and central's view reads name it. Today
//! it keeps the journaled settlement task; its own rows replace that next.

use durable_steps::{
    AtomicStore,
    storage::{JournalStorage, TransactionScope},
};
use fleet_project_lease::{LeaseError, SettlementState};
use fleet_protocol::{ClaimSettlement, TransferId};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::future::Future;

/// Central's settlement owner, for the lease rules and the holder-change view.
pub(crate) struct CentralSettlement;

impl<S, D> fleet_project_lease::OwedSettlement<S, D> for CentralSettlement
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
        fleet_project_lease::journaled_owe_settlement::<S, D>(scope, transfer, settlement, now)
    }
}

impl fleet_project_lease::SettlementStateSource for CentralSettlement {
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
        fleet_project_lease::journaled_settlement_state(scope, transfer)
    }
}

impl<Store, S, D> fleet_project_lease::OwedSettlement<S, D>
    for crate::fleet_lease_projection_step::FleetLeaseChangeHook<Store>
where
    Store: Send + Sync + 'static,
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
        <CentralSettlement as fleet_project_lease::OwedSettlement<S, D>>::owe_settlement(
            scope, transfer, settlement, now,
        )
    }
}
