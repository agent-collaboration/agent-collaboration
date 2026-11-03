//! Terminal status and cold custody are observed through the public SDK snapshot.
use crate::{
    messaging_stopped_owner,
    owned_host::{BoardHost, StoppedHost},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::time::Duration;

pub async fn terminal(host: &BoardHost, sender: &Value, item: &str) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let shown = host
                .mcp("router_show", json!({"caller":sender,"reference":item}))
                .await?;
            let status = shown
                .pointer("/mail/status")
                .ok_or_else(|| format!("no mail status: {shown}"))?;
            if matches!(
                status.get("kind").and_then(Value::as_str),
                Some("delivered" | "refused" | "cancelled" | "expired" | "uncertain")
            ) {
                return Ok(status.clone());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn require_delivered(host: &BoardHost, sender: &Value, item: &str) -> FixtureResult {
    assert_eq!(
        terminal(host, sender, item).await?.get("kind"),
        Some(&json!("delivered"))
    );
    Ok(())
}

pub async fn require_cold_terminal(stopped: &StoppedHost, item: &str) -> FixtureResult {
    let rows = messaging_stopped_owner::delivery_rows(stopped).await?;
    let header = messaging_stopped_owner::header(&rows, "outbox", item)?;
    assert_eq!(header.get("kind"), Some(&json!("item")));
    let header = header.get("value").ok_or("SDK Outbox item header absent")?;
    assert_eq!(header.get("id"), Some(&json!(item)));
    assert_eq!(header.pointer("/status/kind"), Some(&json!("delivered")));
    let _: fleet_sync_protocol::ItemDigest = serde_json::from_value(
        header
            .get("digest")
            .ok_or("retained digest absent")?
            .clone(),
    )?;
    Ok(())
}
