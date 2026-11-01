//! Named fixture preparation only; every observation still uses the real decoder.
use crate::generated_automation_fixture as queries;
use collaboration_protocol::{AutomationId, BatchId, ContentId, FiringId, RunId};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationFixtureMutation {
    ContentText {
        content_id: ContentId,
        text: String,
    },
    FiringOutcome {
        firing_id: FiringId,
        report_seq: u64,
        outcome_json: String,
    },
    BatchBytes {
        batch_id: BatchId,
        frozen_bytes: String,
    },
    WakeExpiry {
        expires_at_ms: i64,
    },
    ExternalOccupant {
        service_id: ServiceId,
        import_id: durable_steps::Uuid,
        run_id: RunId,
    },
}

pub(super) async fn apply<Scope: TransactionScope>(
    scope: &mut Scope,
    automation: &AutomationId,
    mutation: AutomationFixtureMutation,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let input = match &mutation {
        AutomationFixtureMutation::ContentText { content_id, text } => serde_json::json!({
            "automationId":automation,"contentId":content_id,"text":text,"digest":digest(text),
        }),
        AutomationFixtureMutation::FiringOutcome { firing_id, report_seq, outcome_json } => serde_json::json!({
            "automationId":automation,"firingId":firing_id,"reportSeq":report_seq,"outcomeJson":outcome_json,
        }),
        AutomationFixtureMutation::BatchBytes { batch_id, frozen_bytes } => serde_json::json!({
            "automationId":automation,"batchId":batch_id,"frozenBytes":frozen_bytes,"digest":digest(frozen_bytes),
        }),
        AutomationFixtureMutation::WakeExpiry { expires_at_ms } => serde_json::json!({
            "automationId":automation,"expiresAtMs":expires_at_ms,
        }),
        AutomationFixtureMutation::ExternalOccupant { service_id, import_id, run_id } => serde_json::json!({
            "automationId":automation,"serviceId":service_id,"importId":import_id,"runId":run_id,
            "digest":digest(&format!("{}:{}:{}",automation.as_str(),service_id.as_str(),run_id.as_str())),
        }),
    }.to_string();
    match mutation {
        AutomationFixtureMutation::ContentText { .. } => {
            queries::fixture_write_automation_content(scope, input)
                .await
                .map_err(StoreError::backend)?
        }
        AutomationFixtureMutation::FiringOutcome { .. } => {
            queries::fixture_write_automation_outcome(scope, input)
                .await
                .map_err(StoreError::backend)?
        }
        AutomationFixtureMutation::BatchBytes { .. } => {
            queries::fixture_write_automation_batch(scope, input)
                .await
                .map_err(StoreError::backend)?
        }
        AutomationFixtureMutation::WakeExpiry { .. } => {
            queries::fixture_write_wake_expiry(scope, input)
                .await
                .map_err(StoreError::backend)?
        }
        AutomationFixtureMutation::ExternalOccupant { .. } => {
            queries::fixture_insert_automation_import(scope, input.clone())
                .await
                .map_err(StoreError::backend)?;
            queries::fixture_write_external_occupant(scope, input)
                .await
                .map_err(StoreError::backend)?;
        }
    }
    Ok(())
}
fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
