//! Native operations whose effects have no Router provider-session contract.

pub(super) fn router_unavailable_reason(method: &str) -> Option<&'static str> {
    Some(match method {
        "command/exec"
        | "command/exec/write"
        | "command/exec/terminate"
        | "command/exec/resize"
        | "thread/shellCommand"
        | "thread/backgroundTerminals/clean"
        | "thread/backgroundTerminals/list"
        | "thread/backgroundTerminals/terminate" => {
            "this endpoint does not provide host shell commands or terminals"
        }
        "thread/fork" | "thread/inject_items" => {
            "provider conversation context cannot be cloned or injected here"
        }
        "thread/name/set" | "thread/archive" | "thread/unarchive" => {
            "provider-session metadata cannot be changed here"
        }
        "thread/queue/add"
        | "thread/queue/list"
        | "thread/queue/update"
        | "thread/queue/delete"
        | "thread/queue/reorder"
        | "thread/queue/start" => "the native thread queue is not exposed on this endpoint",
        "review/start" => "Codex review execution is not a provider-session operation",
        "thread/realtime/start"
        | "thread/realtime/appendAudio"
        | "thread/realtime/appendText"
        | "thread/realtime/appendSpeech"
        | "thread/realtime/stop"
        | "thread/realtime/listVoices" => "provider sessions do not expose Codex realtime audio",
        "account/rateLimits/read" => "Codex account limits do not describe this provider account",
        "account/login/start" | "account/login/cancel" | "account/logout" => {
            "provider authentication is managed outside the Codex login flow"
        }
        "thread/revert" => "provider conversation history cannot be reverted here",
        "thread/memoryMode/set" | "thread/memory/read" | "memory/status" | "memory/reset" => {
            "Codex memory management is not exposed by provider sessions"
        }
        "thread/goal/set" | "thread/goal/get" | "thread/goal/clear" => {
            "Codex thread goals are not exposed by provider sessions"
        }
        _ => return None,
    })
}
