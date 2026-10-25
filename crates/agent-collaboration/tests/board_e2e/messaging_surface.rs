//! Actual CLI/MCP requests and bounded public sender observations.
use super::messaging_fixture::MessagingCase;
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::{ffi::OsStr, time::Duration};

impl MessagingCase<'_> {
    pub fn direct_request(&self, item: &str, text: &str) -> FixtureResult<Value> {
        Ok(json!({"itemId":item,"target":self.recipient_target()?,
            "message":{"kind":"humanUser","text":text},"delivery":"auto"}))
    }

    pub async fn send_cli(&self, item: &str, text: &str) -> FixtureResult<Value> {
        let target = serde_json::to_string(&self.recipient_target()?)?;
        let arguments = [
            "message",
            "send",
            "--human-user",
            "--to",
            target.as_str(),
            "--item-id",
            item,
            "--text",
            text,
        ];
        let output = self
            .sender
            .host()?
            .cli_output(&arguments.iter().map(OsStr::new).collect::<Vec<_>>())
            .await?;
        let response: Value = serde_json::from_slice(&output.stdout)?;
        if !output.status.success() {
            return Err(format!(
                "real send CLI {:?}: {response}; stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        assert_eq!(
            response.pointer("/result/record/queued/item"),
            Some(&json!(item))
        );
        assert_eq!(
            response.pointer("/result/record/queued/target"),
            Some(&json!(self.recipient_target()?))
        );
        Ok(response)
    }

    pub async fn wait_sender_status(&self, item: &str, status: &Value) -> FixtureResult<Value> {
        let mut last = Value::Null;
        let observed = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let shown = self
                    .sender
                    .host()?
                    .mcp_reply(
                        "router_show",
                        json!({
                            "caller":{"kind":"humanUser"},"reference":item
                        }),
                    )
                    .await?;
                last = shown.value;
                if shown.is_error {
                    // A restarted publisher may not have admitted its committed
                    // handoff yet. Only the typed missing-item result is retried.
                    if last.get("serviceKind") != Some(&json!("notFound")) {
                        return Err(format!("sender status read refused: {last}").into());
                    }
                } else if last.pointer("/mail/status") == Some(status) {
                    assert_eq!(last.pointer("/mail/item"), Some(&json!(item)));
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(last.clone());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        match observed {
            Ok(answer) => answer,
            Err(error) => {
                Err(format!("sender {item} expected {status}; last={last}; {error}").into())
            }
        }
    }

    pub async fn cancel(&self, original: &str, cancel: &str) -> FixtureResult<Value> {
        let result = self
            .sender
            .host()?
            .mcp(
                "message_cancel",
                json!({
                    "cancelId":cancel,"target":original,"sender":{"kind":"humanUser"}
                }),
            )
            .await?;
        assert_eq!(result.get("cancel"), Some(&json!(cancel)));
        assert_eq!(result.get("target"), Some(&json!(original)));
        Ok(result)
    }

    pub async fn wait_prevented(&self, original: &str, cancel: &str) -> FixtureResult<Value> {
        let mut last = Value::Null;
        let observed = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                last = self.cancel(original, cancel).await?;
                if last.get("outcome") == Some(&json!({"kind":"prevented"})) {
                    return Ok(last.clone());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        match observed {
            Ok(answer) => answer,
            Err(error) => Err(format!(
                "cancel {cancel} for {original} expected Prevented; last={last}; {error}"
            )
            .into()),
        }
    }
}
