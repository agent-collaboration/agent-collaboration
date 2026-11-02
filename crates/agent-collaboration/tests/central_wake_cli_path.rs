//! The compiled CLI must preserve a foreign Wake target and truthful preparation failures.
#[path = "support/central_wake_cli_pair.rs"]
mod fixture;
use collaboration_protocol::{OperationId, WakeShowRequest};
use fixture::{CentralWakePair, ProofResult};
use serde_json::{Value, json};

#[tokio::test]
async fn compiled_cli_creates_a_central_wake_on_another_enrolled_host() -> ProofResult {
    let hosts = Box::pin(CentralWakePair::start()).await?;
    let proof: ProofResult = async {
        // Arrange: actual A/B service identities; the central clock alone can admit a Firing.
        let target = json!({
            "endpoint": {
                "serviceId": hosts.second.service_id(),
                "endpointId": "codex-local"
            },
            "sessionId": "foreign-wake-recipient"
        });
        let operation = OperationId::generate();
        // Act: public compiled CLI on A, exact foreign SessionRef, real machine session and central.
        let output = hosts
            .run_cli(
                &[
                    "wake",
                    "send",
                    "--to",
                    &target.to_string(),
                    "--at",
                    &hosts.due,
                    "--for",
                    "2h",
                    "--text",
                    "Cross-machine central Wake",
                    "--operation-id",
                    operation.as_str(),
                ],
                "wake-cli-a",
            )
            .await?;
        let response: Value = serde_json::from_slice(&output.stdout)?;
        if !output.status.success() {
            return Err(format!(
                "A-to-B CLI Wake failed: status={}; {response}; {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        // Assert: preserve central's actual identity and the original A sender, with no Local copy.
        if response.pointer("/result/record/kind") != Some(&json!("central"))
            || response.get("operationId").is_some()
            || response.pointer("/result/effects/operationId") != Some(&json!(operation))
        {
            return Err(format!("wrong central envelope: {response}").into());
        }
        let wake = response
            .pointer("/result/record/value")
            .ok_or("central Wake absent")?;
        if wake.pointer("/definition/message/target") != Some(&target)
            || wake.pointer("/definition/message/content/sender/endpoint/serviceId")
                != Some(&json!(hosts.first.service_id()))
            || wake.get("firstFiring") != Some(&Value::Null)
        {
            return Err("CLI changed target, creator or invented Firing".into());
        }
        let id = serde_json::from_value(
            wake.pointer("/definition/wakeupId")
                .ok_or("Wake identity")?
                .clone(),
        )?;
        let client = collaboration_client::CollaborationClient::connect(
            &hosts.second_root,
            "cross-machine-readback",
            "1",
        )
        .await?;
        let shown = client
            .read_wakeup(WakeShowRequest { wakeup_id: id })
            .await?;
        if serde_json::to_value(shown)? != response["result"]["record"] {
            return Err("B did not read the same central Wake".into());
        }
        use sqlx::Connection as _;
        for root in [&hosts.first_root, &hosts.second_root] {
            let options = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(root.join("automation.sqlite"))
                .read_only(true);
            let mut sql = sqlx::SqliteConnection::connect_with(&options).await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wakeup_definitions")
                .fetch_one(&mut sql)
                .await?;
            sql.close().await?;
            if count != 0 {
                return Err("new cross-machine Wake wrote a local definition".into());
            }
        }
        Ok(())
    }
    .await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn sender_preparation_failure_is_typed_and_never_claims_unknown_submission() -> ProofResult {
    let hosts = Box::pin(CentralWakePair::start()).await?;
    let proof: ProofResult = async {
        // An oversized harness ID passes harness discovery, then fails SessionRef construction.
        let oversized = "x".repeat(collaboration_protocol::limits::SESSION_ID_MAX_BYTES + 1);
        let target = json!({
            "endpoint": {
                "serviceId": hosts.first.service_id(),
                "endpointId": "codex-local"
            },
            "sessionId": "local-recipient"
        }).to_string();
        let operation = OperationId::generate();
        let output = hosts.run_cli(
            &[
                "wake", "send", "--to", &target,
                "--at", &hosts.due, "--for", "2h", "--text", "Never dispatched",
                "--operation-id", operation.as_str(),
            ],
            &oversized,
        ).await?;
        let response: Value = serde_json::from_slice(&output.stdout)?;
        if output.status.code() != Some(2)
            || response.get("operationId") != Some(&json!(operation))
            || response.pointer("/error/kind") != Some(&json!("invalidField"))
            || response.pointer("/error/message").and_then(Value::as_str)
                .is_none_or(|message| !message.contains("CODEX_THREAD_ID"))
        {
            return Err(format!(
                "preparation refusal lost its typed cause or invented uncertainty: {response}; exit={}",
                output.status
            ).into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}
