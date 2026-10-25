//! Prune the retained feed and advance its floor in the same Fleet transaction.
use crate::step_driver::{StepCallback, StepResult};
use durable_steps::{AtomicStore, BoxFuture, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct PruneFeed {
    pub now: chrono::DateTime<chrono::Utc>,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for PruneFeed
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<(), Infallible>> {
        Box::pin(async move {
            crate::fleet_feed_store::prune(scope, self.now).await?;
            Ok(((), Vec::new()))
        })
    }
}
