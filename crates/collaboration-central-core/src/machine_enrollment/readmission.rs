//! Readmission activates its reserved enrollment after the earlier revoke finishes.

use super::{
    AtomicStore, ColumnIndex, Database, Decode, Deserialize, Encode, IntoArguments, PeerId,
    Serialize, StoreError, TransactionScope, Type, queries,
};
use durable_steps::{OperationContext, OperationFailure, Task};
use std::marker::PhantomData;

pub(super) const READMISSION_TASK_KIND: &str = "central.readmit-machine";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadmissionInput {
    pub peer: PeerId,
}
pub(crate) struct ReadmissionTask<Store, Driver> {
    pub marker: PhantomData<fn() -> (Store, Driver)>,
}
impl<Store: AtomicStore, Driver: Database> Task for ReadmissionTask<Store, Driver>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    const KIND: &'static str = READMISSION_TASK_KIND;
    const VERSION: u32 = 2;
    type Store = Store;
    type Input = ReadmissionInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &ReadmissionInput,
    ) -> Result<(), OperationFailure> {
        let peer = input.peer;
        let updated_at = chrono::DateTime::from_timestamp_millis(context.now().unix_millis())
            .ok_or_else(|| invalid_step("readmission clock is outside the timestamp range"))?
            .to_rfc3339();
        context
            .local(
                durable_steps::StepName::new("activate_enrollment").map_err(invalid_step)?,
                move |scope| {
                    Box::pin(async move {
                        queries::activate_enrollment(scope, updated_at, peer.to_string())
                            .await
                            .map_err(StoreError::backend)?;
                        Ok(())
                    })
                },
            )
            .await?;
        Ok(())
    }
}
fn invalid_step(error: impl std::fmt::Display) -> OperationFailure {
    OperationFailure::InvalidPayload {
        message: error.to_string(),
    }
}
