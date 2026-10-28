//! Read a Claim's owning Fleet row through the production decoder.

use crate::{
    ClaimReservationState,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, TransferId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(super) struct ReadClaimReservation {
    pub database: DatabaseId,
    pub attempt: TransferId,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadClaimReservation
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<ClaimReservationState>;
    type Effect = Infallible;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let reservation =
                crate::fleet_claim_step::read(scope, self.database, self.attempt).await?;
            Ok((reservation, Vec::new()))
        })
    }
}
