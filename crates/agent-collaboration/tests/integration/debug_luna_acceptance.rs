//! Opt-in live proof. Run only against automation-debug-host, never production or an existing thread.
mod proof_context {
    pub(super) use crate::proof_context::*;
    mod native_notification_diagnostics {
        mod tests {
            #[test]
            fn retains_retry_class_without_provider_body_or_unrelated_thread_data() {
                crate::proof_context::native_notification_diagnostics::tests::retains_retry_class_without_provider_body_or_unrelated_thread_data()
            }

            #[test]
            fn unknown_provider_payload_is_not_copied_into_diagnostics() {
                crate::proof_context::native_notification_diagnostics::tests::unknown_provider_payload_is_not_copied_into_diagnostics()
            }

            #[test]
            fn underlying_transport_hints_do_not_retain_urls_or_error_bodies() {
                crate::proof_context::native_notification_diagnostics::tests::underlying_transport_hints_do_not_retain_urls_or_error_bodies()
            }
        }
    }
}
use collaboration_client::protocol::{
    DeliveryClientReceipt, MessageContent, MessageDelivery, NativeSendReceipt,
    SessionMessageSendParams,
};
use proof_context::{ProofContext, ProofResult};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires an isolated debug Host; submits one minimal Luna turn and observes its history"]
async fn fresh_native_history_becomes_readable_without_resubmission() -> ProofResult<()> {
    let mut proof = ProofContext::connect().await?;
    let target = proof.start_thread("Luna history readiness probe").await?;
    let thread_id = String::from(target.session_id.clone());
    let database_path = std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?)
        .join(".codex/state_5.sqlite");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database_path)
        .read_only(true)
        .create_if_missing(false);
    let metadata = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let receipt = proof
        .client
        .send_agent_message(SessionMessageSendParams {
            item_id: None,
            target: target.clone(),
            generation_guard: Some(proof.generation.clone()),
            message: MessageContent::Agent {
                sender: target.clone(),
                text: "Output exactly HISTORY_READY. Do not call tools or other agents."
                    .to_owned()
                    .try_into()?,
            },
            mode: MessageDelivery::Auto,
        })
        .await?;
    let collaboration_client::protocol::MessageSendResult::Local(receipt) = receipt else {
        return Err("Local history probe returned remote sender custody".into());
    };
    let receipt = receipt.receipt;
    let turn_id = match &receipt.client {
        Some(DeliveryClientReceipt::CodexAppServer(NativeSendReceipt {
            acceptance:
                collaboration_client::protocol::NativeSendAcceptance::NativeInputAccepted {
                    turn_id,
                    ..
                },
            ..
        })) => String::from(turn_id.clone()),
        _ => return Err("Fresh history probe did not receive an exact native turn".into()),
    };
    proof.record("historyProbeInputAccepted", json!(receipt))?;
    let started = tokio::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        let history_mode: Option<String> =
            sqlx::query_scalar("SELECT history_mode FROM threads WHERE id = ?")
                .bind(&thread_id)
                .fetch_optional(&metadata)
                .await?;
        let response = proof
            .native
            .request_validated(
                &proof.schemas,
                codex_native_integration::NativeOperation::ReadThread,
                json!({"threadId":thread_id,"includeTurns":true}),
            )
            .await;
        match response {
            Ok(response) => {
                if response.pointer("/thread/id").and_then(Value::as_str)
                    != Some(thread_id.as_str())
                {
                    return Err("History probe returned a different native thread".into());
                }
                let turn = response
                    .pointer("/thread/turns")
                    .and_then(Value::as_array)
                    .and_then(|turns| {
                        turns.iter().find(|turn| {
                            turn.get("id").and_then(Value::as_str) == Some(turn_id.as_str())
                        })
                    });
                proof.record(
                    "historyReadinessObserved",
                    json!({
                        "elapsedMs":started.elapsed().as_millis(),"historyMode":history_mode,
                        "target":target,"turn":turn
                    }),
                )?;
                if turn.is_some_and(|turn| {
                    turn.get("status").and_then(Value::as_str) == Some("completed")
                        && proof_context::agent_text(turn).contains("HISTORY_READY")
                }) {
                    metadata.close().await;
                    return Ok(());
                }
            }
            Err(error) => {
                let rejection = proof.native.take_last_rejection();
                proof.record(
                    "historyReadinessRejected",
                    json!({
                        "elapsedMs":started.elapsed().as_millis(),"historyMode":history_mode,
                        "target":target,"rejection":rejection
                    }),
                )?;
                if !matches!(
                    error,
                    codex_native_integration::NativeConnectionError::Rejected { .. }
                ) {
                    return Err(error.into());
                }
            }
        }
        if started.elapsed() >= std::time::Duration::from_secs(45) {
            return Err("Exact fresh turn history was not readable and completed within 45 seconds; no input was resent".into());
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated debug Host; reads only a thread created by an earlier proof"]
async fn recorded_test_history_is_readable_without_resume() -> ProofResult<()> {
    let mut proof = ProofContext::connect().await?;
    let source = std::path::PathBuf::from(
        std::env::var_os("CODEX_AUTOMATION_SOURCE_PROOF_ROOT")
            .ok_or("source proof root required")?,
    )
    .canonicalize()?;
    if source.parent() != proof.root.parent() {
        return Err("Source must be a sibling private proof root under /tmp".into());
    }
    let context: Value =
        serde_json::from_slice(&std::fs::read(source.join("debug-host-context.json"))?)?;
    if context.get("profile").and_then(Value::as_str) != Some("codex-router-debug")
        || context.get("model").and_then(Value::as_str) != Some("gpt-5.6-luna")
    {
        return Err("Source was not a Luna debug proof".into());
    }
    let events = std::fs::read_to_string(source.join("proof-events.jsonl"))?;
    let target: collaboration_client::protocol::SessionRef = events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| {
            event.get("event").and_then(Value::as_str) == Some("freshThread")
                && event.pointer("/details/role").and_then(Value::as_str) == Some("Luna A")
        })
        .and_then(|event| event.pointer("/details/target").cloned())
        .map(serde_json::from_value)
        .transpose()?
        .ok_or("fresh Alpha identity missing")?;
    let result = proof
        .native
        .request_validated(
            &proof.schemas,
            codex_native_integration::NativeOperation::ReadThread,
            json!({"threadId":String::from(target.session_id.clone()),"includeTurns":true}),
        )
        .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let rejection = proof.native.take_last_rejection();
            proof.record(
                "threadReadRejected",
                json!({"target":target,"rejection":rejection}),
            )?;
            return Err(error.into());
        }
    };
    if result.pointer("/thread/id").and_then(Value::as_str)
        != Some(String::from(target.session_id.clone()).as_str())
        || result
            .pointer("/thread/turns")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err("thread/read did not return the exact saved test thread and its turns".into());
    }
    proof.record(
        "readOnlySavedHistoryVerified",
        json!({"target":target,"response":result}),
    )?;
    Ok(())
}
