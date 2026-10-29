//! Observe the child Host's actual joined machine-session operations.
use super::super::owned_host::BoardHost;
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub(in super::super) async fn observe(host: &BoardHost, after: Option<u64>) -> FixtureResult<u64> {
    let reply = host
        .query_project(json!({"kind":"machineProgress", "after":after}))
        .await?;
    if reply.get("kind") != Some(&json!("machineProgress")) {
        return Err("owned Host machine progress identity mismatch".into());
    }
    reply
        .get("completed")
        .and_then(Value::as_u64)
        .ok_or_else(|| "owned Host omitted machine progress counter".into())
}
