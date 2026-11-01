//! Delivery's journal append receipt advances central notice custody without changing Batch bytes.
use crate::{
    generated_automation_admission as queries, machine_notice_calls::DeliverMachineNotice,
};
use board_sync_protocol::BoardNotice;
use collaboration_protocol::BatchId;
use durable_steps::{
    AtomicStore, OperationContext, OperationFailure, StepName, StoreError, Task,
    storage::TransactionScope,
};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};
use std::marker::PhantomData;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutomationNoticeInput {
    pub installation: ServiceId,
    pub batch_id: BatchId,
}
pub(crate) struct AutomationNoticeTask<Store, Driver>(pub PhantomData<fn() -> (Store, Driver)>);
impl<Store: AtomicStore, Driver: Database> Task for AutomationNoticeTask<Store, Driver>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
{
    const KIND: &'static str = "central.automation-batch-notice";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = AutomationNoticeInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &AutomationNoticeInput,
    ) -> Result<(), OperationFailure> {
        context
            .call(
                step("deliver_notice")?,
                DeliverMachineNotice {
                    installation: input.installation.clone(),
                },
                BoardNotice::AutomationBatchReady {
                    batch_id: input.batch_id.clone(),
                },
            )
            .await?;
        let batch = input.batch_id.clone();
        context
            .local(step("record_custody")?, move |scope| {
                Box::pin(async move {
                    queries::hold_automation_batch(scope, batch.as_str().into())
                        .await
                        .map_err(StoreError::backend)?;
                    queries::hold_automation_firings(scope, batch.as_str().into())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }
}
fn step(name: &str) -> Result<StepName, OperationFailure> {
    StepName::new(name).map_err(|error| OperationFailure::InvalidPayload {
        message: error.to_string(),
    })
}
