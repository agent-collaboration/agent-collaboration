//! A real Fleet alarm evaluates all due Automations in one rollback boundary.
use crate::{
    automation_catalog_store, automation_due_evaluation,
    automation_store_error::{AutomationStoreError, required, timestamp_millis},
    fleet_row_ids::FleetRowIds,
    generated_automation_admission as writes, generated_automation_catalog as reads,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct EvaluateAutomations {
    pub now_ms: i64,
    pub row_ids: FleetRowIds,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for EvaluateAutomations
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
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<(), Infallible>> {
        Box::pin(async move {
            crate::automation_batch_retention::prune(scope, self.now_ms).await?;
            let rows = reads::list_automations(scope, String::new(), i64::MAX.to_string())
                .await
                .map_err(StoreError::backend)?;
            let mut ready = crate::automation_waiting_release::due(scope, self.now_ms)
                .await
                .map_err(StoreError::backend)?;
            for row in rows {
                let id = required(row.automation_id.clone(), "automation_id")
                    .map_err(StoreError::backend)?;
                let automation_id: collaboration_protocol::AutomationId =
                    id.clone().try_into().map_err(StoreError::backend)?;
                // The independently stored expiry runs before definition decode
                // and eligibility, including for a paused or faulted Wake.
                if row.kind.as_deref() == Some("wake")
                    && matches!(
                        row.state.as_deref(),
                        Some("active" | "paused" | "suspended")
                    )
                {
                    match timestamp_millis(row.expires_at_ms.clone(), "expires_at_ms") {
                        Ok(expiry) if expiry <= self.now_ms => {
                            writes::expire_automation_wake(scope, id.clone())
                                .await
                                .map_err(StoreError::backend)?;
                            writes::clear_automation_due(scope, id.clone())
                                .await
                                .map_err(StoreError::backend)?;
                            crate::automation_pending_mutation::cancel(scope, &automation_id)
                                .await?;
                            continue;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            if row.fault_json.is_none() {
                                record_fault(scope, &id, error, self.now_ms).await?;
                            }
                            continue;
                        }
                    }
                }
                if row.fault_json.is_some()
                    || !matches!(row.state.as_deref(), Some("enabled" | "active"))
                {
                    continue;
                }
                let prepared = async {
                    let record = automation_catalog_store::decode(row)?;
                    automation_due_evaluation::prepare(
                        scope,
                        record,
                        self.now_ms,
                        &mut self.row_ids,
                    )
                    .await
                }
                .await;
                match prepared {
                    Ok(prepared) => {
                        ready.extend(automation_due_evaluation::commit(scope, prepared).await?)
                    }
                    Err(AutomationStoreError::Storage(error)) => return Err(error),
                    Err(error) => {
                        record_fault(scope, &id, error, self.now_ms).await?;
                    }
                }
            }
            crate::automation_batch_freeze::freeze::<Store, Driver>(
                scope,
                ready,
                self.now_ms,
                &mut self.row_ids,
            )
            .await?;
            crate::automation_clock_deadline::request_next(scope).await?;
            Ok(((), Vec::new()))
        })
    }
}

async fn record_fault<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &str,
    error: AutomationStoreError,
    now_ms: i64,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let fault = serde_json::to_string(&error.into_refusal()?).map_err(StoreError::backend)?;
    writes::record_automation_fault(scope, fault, now_ms.to_string(), id.into())
        .await
        .map_err(StoreError::backend)?;
    writes::clear_automation_due(scope, id.into())
        .await
        .map_err(StoreError::backend)?;
    Ok(())
}
