//! Fleet feed maintenance steps: step 0 creates the feed state row.
use crate::step_driver::{StepCallback, StepResult};
use durable_steps::{AtomicStore, BoxFuture, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

/// Step 0: create the one feed state row if it is missing.
pub(crate) struct EnsureFeedState {
    pub id: durable_steps::Uuid,
    pub now: chrono::DateTime<chrono::Utc>,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for EnsureFeedState
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
            crate::fleet_feed_store::ensure_state(scope, self.id, self.now).await?;
            Ok(((), Vec::new()))
        })
    }
}
