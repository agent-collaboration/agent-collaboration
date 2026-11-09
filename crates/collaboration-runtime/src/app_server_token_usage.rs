//! ACP context occupancy projection for the Codex app-server face.

use serde::Deserialize;
use serde_json::{Value, json};
use session_event_model::SessionItem;

#[derive(Deserialize)]
struct AcpContextUsage {
    used: i64,
    size: i64,
}

pub(crate) fn translate_usage_update(
    item: &SessionItem,
    thread_id: &str,
    turn_id: Option<&str>,
) -> Option<Value> {
    // Codex requires a real turnId; session-level usage cannot supply one.
    let turn_id = turn_id.filter(|turn_id| !turn_id.is_empty())?;
    let usage: AcpContextUsage = serde_json::from_str(item.text.as_deref()?).ok()?;
    if usage.used < 0 || usage.size <= 0 {
        return None;
    }
    // Codex 360319d8 tui/token_usage.rs uses last.totalTokens for occupancy.
    // ACP supplies no input/output/cache split or cumulative billable count.
    let breakdown = json!({
        "totalTokens":usage.used,"inputTokens":0,"cachedInputTokens":0,
        "cacheWriteInputTokens":0,"outputTokens":0,"reasoningOutputTokens":0
    });
    Some(json!({"method":"thread/tokenUsage/updated","params":{
        "threadId":thread_id,"turnId":turn_id,
        "tokenUsage":{"total":breakdown,"last":breakdown,"modelContextWindow":usage.size}
    }}))
}

#[cfg(test)]
#[path = "app_server_token_usage_test.rs"]
mod tests;
