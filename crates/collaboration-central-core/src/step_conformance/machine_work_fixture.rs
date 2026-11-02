//! Owner-only fault preparation observes exact stored bytes without product decoding.
use crate::{
    generated_machine_work_fixture as queries,
    step_driver::{StepCallback, StepResult},
};
use collaboration_protocol::{BatchId, WorkItemId};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineWorkFixtureMutation {
    RequestJson {
        request_json: String,
    },
    RequestDigest {
        request_digest: String,
    },
    State {
        state: String,
    },
    ResultJson {
        result_json: Option<String>,
    },
    FrozenBatch {
        batch_id: BatchId,
        frozen_bytes: String,
    },
}

pub(super) struct MachineWorkFixture {
    pub work_item_id: WorkItemId,
    pub mutation: Option<MachineWorkFixtureMutation>,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for MachineWorkFixture
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = serde_json::Value;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let id = self.work_item_id.as_str().to_owned();
            match self.mutation {
                Some(MachineWorkFixtureMutation::RequestJson { request_json }) => {
                    queries::fixture_write_machine_work_request(scope, request_json, id.clone())
                        .await
                        .map_err(StoreError::backend)?;
                }
                Some(MachineWorkFixtureMutation::RequestDigest { request_digest }) => {
                    queries::fixture_write_machine_work_digest(scope, request_digest, id.clone())
                        .await
                        .map_err(StoreError::backend)?;
                }
                Some(MachineWorkFixtureMutation::State { state }) => {
                    queries::fixture_write_machine_work_state(scope, state, id.clone())
                        .await
                        .map_err(StoreError::backend)?;
                }
                Some(MachineWorkFixtureMutation::ResultJson { result_json }) => {
                    queries::fixture_write_machine_work_result(scope, result_json, id.clone())
                        .await
                        .map_err(StoreError::backend)?;
                }
                Some(MachineWorkFixtureMutation::FrozenBatch {
                    batch_id,
                    frozen_bytes,
                }) => {
                    require_selected_entry(&frozen_bytes, &id)?;
                    let input=serde_json::json!({"workItemId":id,"batchId":batch_id,
                        "digest":format!("{:x}",Sha256::digest(frozen_bytes.as_bytes())),"frozenBytes":frozen_bytes}).to_string();
                    queries::fixture_write_machine_work_batch(scope, input)
                        .await
                        .map_err(StoreError::backend)?;
                }
                None => {}
            }
            let observed = queries::fixture_read_machine_work(scope, id)
                .await
                .map_err(StoreError::backend)?
                .and_then(|row| row.observation)
                .ok_or_else(|| StoreError::backend("fixture machine work not found"))?;
            let observed = serde_json::from_str(&observed).map_err(StoreError::backend)?;
            Ok((observed, Vec::new()))
        })
    }
}

fn require_selected_entry(bytes: &str, work_item: &str) -> Result<(), StoreError> {
    let raw: serde_json::Value = serde_json::from_str(bytes).map_err(StoreError::backend)?;
    let selected = raw
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.get("kind").and_then(serde_json::Value::as_str) == Some("sessionCreate")
                    && entry
                        .pointer("/value/workItemId")
                        .and_then(serde_json::Value::as_str)
                        == Some(work_item)
            })
        });
    if !selected {
        return Err(StoreError::backend(
            "fixture Batch must retain selected work item identity",
        ));
    }
    Ok(())
}
