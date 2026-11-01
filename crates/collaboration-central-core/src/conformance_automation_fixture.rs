//! Debug-only fault preparation and raw observations over the real Fleet owner.
use crate::{
    generated_automation_fixture as queries,
    step_driver::{StepCallback, StepResult},
};
use collaboration_protocol::AutomationId;
use durable_steps::{
    AtomicStore, BoxFuture, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;
mod mutation;
pub use mutation::AutomationFixtureMutation;

pub(crate) struct AutomationFixture {
    pub id: AutomationId,
    pub definition_json: Option<String>,
    pub mutation: Option<AutomationFixtureMutation>,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AutomationFixture
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = serde_json::Value;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            if let Some(json) = self.definition_json {
                queries::fixture_write_automation_definition(scope, json, self.id.as_str().into())
                    .await
                    .map_err(StoreError::backend)?;
            }
            if let Some(mutation) = self.mutation {
                mutation::apply(scope, &self.id, mutation).await?;
            }
            let row = queries::fixture_read_automation(scope, self.id.as_str().into())
                .await
                .map_err(StoreError::backend)?
                .ok_or_else(|| StoreError::backend("fixture Automation not found"))?;
            let value = row
                .observation
                .ok_or_else(|| StoreError::backend("fixture observation missing"))?;
            let mut observation: serde_json::Value =
                serde_json::from_str(&value).map_err(StoreError::backend)?;
            observation
                .as_object_mut()
                .ok_or_else(|| StoreError::backend("fixture observation is not an object"))?
                .insert(
                    "journalEarliestMs".into(),
                    serde_json::json!(scope.earliest().await?.map(|at| at.unix_millis())),
                );
            Ok((observation, Vec::new()))
        })
    }
}
