//! Fleet metadata reads, pure decisions and application use one local step.

use crate::{
    fleet_scope_apply, fleet_scope_snapshot,
    step_driver::{StepCallback, StepResult},
};
use chrono::{DateTime, Utc};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_admission::{
    FleetAdmissionError, FleetImportBundle, FleetImportResult, FleetRequest, FleetRequestResult,
};
use message_board::ActorOrigin;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct ImportFleet {
    pub bundle: FleetImportBundle,
    pub now: DateTime<Utc>,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}
pub(crate) struct DecideFleet {
    pub request: FleetRequest,
    pub origin: ActorOrigin,
    pub now: DateTime<Utc>,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ImportFleet
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = FleetImportResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
            let snapshot =
                fleet_scope_snapshot::read(scope, fleet_admission::reads_for_import(&self.bundle))
                    .await?;
            let decision = fleet_admission::decide_import(&self.bundle, &snapshot)
                .map_err(StoreError::backend)?;
            fleet_scope_apply::import(scope, &decision, self.now, &mut self.row_ids).await?;
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(self.now.timestamp_millis()),
            )
            .await?;
            Ok((decision.result, Vec::new()))
        })
    }
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for DecideFleet
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = FleetRequestResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
            let snapshot =
                fleet_scope_snapshot::read(scope, fleet_admission::reads_for(&self.request))
                    .await?;
            let decision =
                match fleet_admission::decide(&self.request, &snapshot, self.now, &self.origin) {
                    Ok(decision) => decision,
                    Err(FleetAdmissionError::ConflictingRequest) => {
                        return Ok((FleetRequestResult::ConflictingRequest, Vec::new()));
                    }
                    Err(error) => return Err(StoreError::backend(error)),
                };
            let result =
                fleet_scope_apply::decision(scope, decision, self.now, &mut self.row_ids).await?;
            if matches!(
                result,
                FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
            ) {
                crate::fleet_archive_completion::complete_request(
                    scope,
                    &self.request,
                    self.now,
                    &mut self.row_ids,
                )
                .await?;
            }
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(self.now.timestamp_millis()),
            )
            .await?;
            Ok((result, Vec::new()))
        })
    }
}
