//! Entity fixture writes reuse the canonical owner and its atomic feed append.
use crate::{
    fleet_record_store::{self, RecordWriteSource},
    fleet_row_ids::FleetRowIds,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use message_board::{Record, RecordPlacement};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct WriteFleetEntityFixture {
    pub record: Record,
    pub row_ids: FleetRowIds,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for WriteFleetEntityFixture
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Record;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let time = self
                .record
                .envelope()
                .time
                .as_router_time()
                .ok_or_else(|| StoreError::Aborted {
                    reason: "entity size fixtures require Router time".into(),
                })?;
            let RecordPlacement::Fleet(placement) = &mut self.record.envelope_mut().position else {
                return Err(StoreError::Aborted {
                    reason: "entity size fixtures require Fleet placement".into(),
                });
            };
            placement.position = crate::fleet_feed_store::high_water(scope)
                .await?
                .successor()
                .map_err(StoreError::backend)?;
            let position = placement.position;
            fleet_record_store::put(
                scope,
                &self.record,
                RecordWriteSource::Live,
                &mut self.row_ids,
            )
            .await?;
            crate::fleet_feed_store::append(
                scope,
                &[fleet_admission::FleetFeedEntry {
                    position,
                    record: self.record.clone(),
                }],
                time,
                &mut self.row_ids,
            )
            .await?;
            Ok((self.record, Vec::new()))
        })
    }
}
