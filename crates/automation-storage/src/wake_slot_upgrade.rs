//! Upgrade existing reminder definitions before committing the unique-slot migration.
use crate::{StorageError, wake_limits, wakeup_evaluation};
use agent_automation::{WakeDefinition, WakeupId};
use collaboration_protocol::{SavedMessage, WakeCreator};
use sqlx::SqliteConnection;
use std::collections::HashSet;

struct WakeUpgradeRow {
    wakeup_id: Option<String>,
    wakeup_status: String,
    definition_json: String,
}
struct ReceiptUpgradeRow {
    operation_id: Option<String>,
    method_name: String,
    final_result_json: Option<String>,
}

pub(crate) async fn apply(
    connection: &mut SqliteConnection,
    release_ms: i64,
) -> Result<(), StorageError> {
    let release =
        chrono::DateTime::from_timestamp_millis(release_ms).ok_or(StorageError::InvalidRecord)?;
    let rows = sqlx::query_as!(WakeUpgradeRow, "SELECT wakeup_id,wakeup_status,definition_json FROM wakeup_definitions ORDER BY created_at_ms DESC,wakeup_id DESC").fetch_all(&mut *connection).await?;
    let mut occupying_targets = HashSet::new();
    for row in rows {
        let id: WakeupId = row
            .wakeup_id
            .ok_or(StorageError::InvalidWake {
                field: "wakeupId",
                reason: "legacy wake identity must be a canonical UUIDv7",
            })?
            .try_into()
            .map_err(|_| StorageError::InvalidWake {
                field: "wakeupId",
                reason: "legacy wake identity must be a canonical UUIDv7",
            })?;
        let old_status = row.wakeup_status;
        let state: agent_automation::WakeState = serde_json::from_value(serde_json::Value::String(
            old_status.clone(),
        ))
        .map_err(|_| StorageError::InvalidWake {
            field: "state",
            reason: "legacy wake state is invalid",
        })?;
        let wire: serde_json::Value =
            serde_json::from_str(&row.definition_json).map_err(|_| invalid_definition())?;
        let mut definition = legacy_definition(wire)?;
        if definition.wakeup_id != id {
            return Err(StorageError::InvalidWake {
                field: "wakeupId",
                reason: "legacy wake definition does not match its row",
            });
        }
        let occupying = matches!(
            state,
            agent_automation::WakeState::Active | agent_automation::WakeState::Paused
        );
        let target = serde_json::to_string(&definition.message.target)
            .map_err(|_| StorageError::InvalidRecord)?;
        let mut reason = None;
        if occupying {
            if !occupying_targets.insert(target) {
                reason = Some("upgradeDuplicate");
            } else {
                let repeating = wake_limits::is_repeating(&definition.timing);
                let seconds = if repeating {
                    wake_limits::REPEAT_MAX
                } else {
                    wake_limits::ONE_SHOT_MAX
                };
                let cap = release_ms
                    .checked_add(i64::from(seconds) * 1000)
                    .ok_or(StorageError::InvalidRecord)?;
                let end_ms = definition.expires_at_ms.map_or(cap, |end| end.min(cap));
                definition.expires_at_ms = Some(end_ms);
                let end = chrono::DateTime::from_timestamp_millis(end_ms)
                    .ok_or(StorageError::InvalidRecord)?;
                match wake_limits::validate_frequency(&definition.timing, release, end) {
                    Err(StorageError::WakeRefused(_)) => reason = Some("upgradeLimit"),
                    result => result?,
                }
            }
        }
        let encoded =
            serde_json::to_string(&definition).map_err(|_| StorageError::InvalidRecord)?;
        sqlx::query!(
            "UPDATE wakeup_definitions SET definition_json=?,expires_at_ms=? WHERE wakeup_id=?",
            encoded,
            definition.expires_at_ms,
            id.as_str()
        )
        .execute(&mut *connection)
        .await?;
        if let Some(reason) = reason {
            wakeup_evaluation::discard_undispatched(connection, &id).await?;
            sqlx::query!("UPDATE wakeup_definitions SET wakeup_status='expired',next_due_at_ms=NULL,updated_at_ms=? WHERE wakeup_id=?", release_ms, id.as_str()).execute(&mut *connection).await?;
            wakeup_evaluation::append_event(connection, wakeup_evaluation::WakeEvent { id: &id, kind: reason, body: &serde_json::json!({"kind":"stateChange","before":old_status,"after":"expired"}), now_ms: release_ms }).await?;
        } else if occupying {
            sqlx::query!("UPDATE wakeup_definitions SET next_due_at_ms=CASE WHEN next_due_at_ms>=expires_at_ms THEN NULL ELSE next_due_at_ms END WHERE wakeup_id=?", id.as_str()).execute(&mut *connection).await?;
        }
    }
    upgrade_operation_answers(connection).await
}

fn invalid_definition() -> StorageError {
    StorageError::InvalidWake {
        field: "definition",
        reason: "legacy wake definition is invalid",
    }
}

fn legacy_definition(
    mut wire: serde_json::Value,
) -> Result<WakeDefinition<SavedMessage, WakeCreator>, StorageError> {
    let object = wire.as_object_mut().ok_or_else(invalid_definition)?;
    if object.contains_key("createdBy") {
        return Err(invalid_definition());
    }
    object.insert("createdBy".into(), serde_json::json!({"kind":"unrecorded"}));
    serde_json::from_value(wire).map_err(|_| invalid_definition())
}

async fn upgrade_operation_answers(connection: &mut SqliteConnection) -> Result<(), StorageError> {
    let invalid = || StorageError::InvalidWake {
        field: "operationResult",
        reason: "legacy wake operation result is invalid",
    };
    let mut after: Option<String> = None;
    loop {
        let rows = sqlx::query_as!(ReceiptUpgradeRow, "SELECT operation_id,method_name,final_result_json FROM operation_receipts WHERE method_name IN ('wake/send','wake/pause','wake/resume','wake/cancel') AND operation_status='succeeded' AND (? IS NULL OR operation_id>?) ORDER BY operation_id LIMIT 128", after, after)
            .fetch_all(&mut *connection).await?;
        if rows.is_empty() {
            return Ok(());
        }
        for row in rows {
            let id = row.operation_id.ok_or_else(invalid)?;
            let method = row.method_name;
            let encoded = row.final_result_json;
            let mut result: serde_json::Value =
                serde_json::from_str(&encoded.ok_or_else(invalid)?).map_err(|_| invalid())?;
            let path = if method == "wake/send" {
                "/definition"
            } else {
                "/wake/definition"
            };
            let definition = result.pointer_mut(path).ok_or_else(invalid)?;
            *definition = serde_json::to_value(legacy_definition(definition.take())?)
                .map_err(|_| invalid())?;
            // Only creator metadata changes. First-answer timing, outcomes and
            // canonical request bytes remain exactly their original values.
            if method == "wake/send" {
                let _: agent_automation::WakeRecord<SavedMessage, WakeCreator> =
                    serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            } else {
                let _: crate::WakeMutationResult<SavedMessage> =
                    serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            }
            let encoded_result = result.to_string();
            sqlx::query!(
                "UPDATE operation_receipts SET final_result_json=? WHERE operation_id=?",
                encoded_result,
                id
            )
            .execute(&mut *connection)
            .await?;
            after = Some(id);
        }
    }
}
