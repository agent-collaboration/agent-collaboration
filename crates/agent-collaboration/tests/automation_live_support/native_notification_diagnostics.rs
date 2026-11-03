//! Keep bounded diagnostics for a test-owned thread without retaining provider error bodies.
use serde_json::{Value, json};

pub(super) fn for_target(notification: &Value, thread_id: &str) -> Option<Value> {
    if notification.get("method").and_then(Value::as_str) != Some("error")
        || notification
            .pointer("/params/threadId")
            .and_then(Value::as_str)
            != Some(thread_id)
    {
        return None;
    }
    let info = notification.pointer("/params/error/codexErrorInfo");
    let known = [
        "contextWindowExceeded",
        "sessionBudgetExceeded",
        "usageLimitExceeded",
        "rateLimitExceeded",
        "serverOverloaded",
        "cyberPolicy",
        "misalignmentPolicyViolation",
        "httpConnectionFailed",
        "responseStreamConnectionFailed",
        "internalServerError",
        "unauthorized",
        "badRequest",
        "threadRollbackFailed",
        "sandboxError",
        "responseStreamDisconnected",
        "responseTooManyFailedAttempts",
        "activeTurnNotSteerable",
        "other",
    ];
    let kind = known
        .into_iter()
        .find(|kind| {
            info.is_some_and(|info| {
                info.as_str() == Some(*kind)
                    || info
                        .as_object()
                        .is_some_and(|object| object.contains_key(*kind))
            })
        })
        .unwrap_or("unclassified");
    let status = info
        .and_then(|info| info.get(kind))
        .and_then(|details| details.get("httpStatusCode"))
        .and_then(Value::as_u64)
        .filter(|status| (100..=599).contains(status));
    let details = notification
        .pointer("/params/error/additionalDetails")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hints: Vec<&str> = [
        ("connection refused", "connectionRefused"),
        ("connection reset", "connectionReset"),
        ("closed by server", "serverClosed"),
        ("response.completed", "missingCompletionEvent"),
        ("timed out", "requestTimeout"),
        ("timeout", "timeout"),
        ("websocket", "websocket"),
        ("error sending request", "requestTransportError"),
        ("certificate", "certificateError"),
        ("dns", "dnsError"),
        ("502", "mentions502"),
        ("503", "mentions503"),
        ("401", "mentions401"),
        ("403", "mentions403"),
        ("429", "mentions429"),
        ("decode", "decodeError"),
        ("parse", "parseError"),
    ]
    .into_iter()
    .filter_map(|(needle, hint)| details.contains(needle).then_some(hint))
    .collect();
    Some(
        json!({"threadId":thread_id,"errorKind":kind,"httpStatusCode":status,
        "willRetry":notification.pointer("/params/willRetry").and_then(Value::as_bool),"detailHints":hints}),
    )
}

#[cfg(test)]
#[path = "native_notification_diagnostics_test.rs"]
pub(crate) mod tests;
