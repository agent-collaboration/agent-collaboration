//! Real-store transaction observations without exposing the production driver.

use crate::{ObjectHost, fleet_decision_step::FleetDecisionFault, step_driver::StepDriver};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::FleetRequestResult;
use message_board::{DecisionEffect, RequestId, RequestLedgerEntry};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

mod claim_reservation;
#[cfg(not(target_arch = "wasm32"))]
#[path = "step_conformance/native_callback.rs"]
mod native_callback;

pub struct StepConformance<Host: ObjectHost> {
    driver: StepDriver<Host>,
    bootstrap: crate::ObjectBootstrap<Host>,
    ids: Arc<dyn runtime_primitives::IdSource>,
}

impl<Host: ObjectHost> StepConformance<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn execute(&self, command: StepControl) -> Result<StepControlReply, StoreError> {
        match command {
            StepControl::Initialize => {
                self.initialize().await?;
                Ok(StepControlReply::Initialized)
            }
            StepControl::Read { request_id } => self
                .read_fleet_decision(request_id)
                .await
                .map(|entry| StepControlReply::Read(entry.map(Box::new))),
            StepControl::ReadClaimReservation { database, attempt } => self
                .driver
                .run(claim_reservation::ReadClaimReservation { database, attempt })
                .await
                .map(|(reservation, _)| StepControlReply::ClaimReservation(reservation)),
            StepControl::Record {
                entry,
                effects,
                fault,
            } => self
                .record_with_fault(*entry, effects, fault)
                .await
                .map(StepControlReply::Recorded),
        }
    }

    pub fn new(
        host: Arc<Host>,
        ids: Arc<dyn runtime_primitives::IdSource>,
        now: runtime_primitives::SyncTime,
    ) -> Self {
        Self {
            driver: StepDriver::new(host.clone()),
            bootstrap: crate::ObjectBootstrap::new(host, now),
            ids,
        }
    }

    pub async fn initialize(&self) -> Result<(), StoreError> {
        self.bootstrap
            .initialize(crate::EngineSchema::Fleet)
            .await?;
        Ok(())
    }

    pub async fn record_fleet_decision(
        &self,
        entry: RequestLedgerEntry,
        effects: Vec<DecisionEffect>,
    ) -> Result<Vec<FleetRequestResult>, StoreError> {
        let receipt_ids = (0..effects.len()).map(|_| self.ids.new_id()).collect();
        Ok(crate::effect_driver::conformance_effects(
            self.driver
                .record_fleet_decision(entry, effects, receipt_ids)
                .await?,
        ))
    }

    pub async fn read_fleet_decision(
        &self,
        request: RequestId,
    ) -> Result<Option<RequestLedgerEntry>, StoreError> {
        self.driver.read_fleet_decision(request).await
    }

    /// Write an entity and its feed row atomically for native storage measurements.
    pub async fn write_fleet_entity_fixture(
        &self,
        record: message_board::Record,
    ) -> Result<message_board::Record, StoreError> {
        let row_ids = crate::fleet_row_ids::FleetRowIds::new(self.ids.clone());
        let (record, _) = self
            .driver
            .run(crate::conformance_fleet_entity::WriteFleetEntityFixture { record, row_ids })
            .await?;
        Ok(record)
    }

    /// Observe the live page rule on the same real store as the checked writer.
    pub async fn read_fleet_effects(
        &self,
        request: message_board::RequestEffectsRequest,
    ) -> Result<message_board::RequestEffectsRead, StoreError> {
        let (reply, _) = self
            .driver
            .run(crate::fleet_effects_read::ReadFleetEffects(request))
            .await?;
        Ok(reply)
    }

    pub async fn record_with_fault(
        &self,
        entry: RequestLedgerEntry,
        effects: Vec<DecisionEffect>,
        fault: FleetDecisionFault,
    ) -> Result<Vec<FleetRequestResult>, StoreError> {
        let receipt_ids = (0..effects.len()).map(|_| self.ids.new_id()).collect();
        Ok(crate::effect_driver::conformance_effects(
            self.driver
                .conformance_record_fleet_decision(entry, effects, receipt_ids, fault)
                .await?,
        ))
    }

    /// Run a native robustness callback after writes, before the transaction returns.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn record_with_native_callback(
        &self,
        entry: RequestLedgerEntry,
        effects: Vec<DecisionEffect>,
        after_write: fn(),
    ) -> Result<Vec<FleetRequestResult>, StoreError> {
        let receipt_ids = (0..effects.len()).map(|_| self.ids.new_id()).collect();
        let (_, committed) = self
            .driver
            .run(native_callback::AfterDecisionWrite {
                decision: crate::fleet_decision_step::FleetDecisionStep {
                    entry,
                    effects,
                    receipt_ids,
                    fault: FleetDecisionFault::None,
                },
                after_write,
            })
            .await?;
        Ok(crate::effect_driver::conformance_effects(committed))
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum StepControl {
    Initialize,
    Read {
        request_id: RequestId,
    },
    ReadClaimReservation {
        database: fleet_sync_protocol::DatabaseId,
        attempt: fleet_sync_protocol::TransferId,
    },
    Record {
        entry: Box<RequestLedgerEntry>,
        effects: Vec<DecisionEffect>,
        fault: FleetDecisionFault,
    },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum StepControlReply {
    Initialized,
    Read(Option<Box<RequestLedgerEntry>>),
    ClaimReservation(Option<crate::ClaimReservationState>),
    Recorded(Vec<FleetRequestResult>),
}
