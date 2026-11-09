//! Native settings and status snapshots projected from existing Session facts.
use serde_json::{Value, json};
use session_event_model::{InteractionKind, SessionSettings, SessionState};
use std::path::Path;

pub(crate) fn render_thread_status(state: &SessionState) -> Value {
    match state {
        SessionState::Unloaded | SessionState::Closed => json!({"type":"notLoaded"}),
        SessionState::Idle => json!({"type":"idle"}),
        SessionState::Running => json!({"type":"active","activeFlags":[]}),
        SessionState::AuthenticationRequired => json!({"type":"systemError"}),
        SessionState::RequiresAction { pending } => {
            let mut active_flags = Vec::new();
            for interaction in pending.iter() {
                let flag = match interaction.kind() {
                    InteractionKind::Approval => "waitingOnApproval",
                    InteractionKind::Question => "waitingOnUserInput",
                };
                if !active_flags.contains(&flag) {
                    active_flags.push(flag);
                }
            }
            json!({"type":"active","activeFlags":active_flags})
        }
    }
}

pub(crate) fn state_notifications(thread_id: &str, state: &SessionState) -> Vec<Value> {
    let mut frames = vec![json!({"method":"thread/status/changed","params":{
        "threadId":thread_id,"status":render_thread_status(state)
    }})];
    if matches!(state, SessionState::Closed) {
        frames.push(json!({"method":"thread/closed","params":{"threadId":thread_id}}));
    }
    frames
}

pub(crate) fn settings_notification(
    thread_id: &str,
    model_provider: &str,
    settings: &SessionSettings,
    cwd: &Path,
) -> Value {
    let model = settings.model.as_deref().unwrap_or("provider-default");
    // Keep the native front door's existing startup policy and Default-mode
    // projection. ACP mode names do not establish Codex collaboration modes.
    json!({"method":"thread/settings/updated","params":{
        "threadId":thread_id,
        "threadSettings":{
            "cwd":cwd,"disabledPluginIds":[],
            "approvalPolicy":"on-request","approvalsReviewer":"user",
            "sandboxPolicy":{"type":"dangerFullAccess"},"activePermissionProfile":null,
            "model":model,"modelProvider":model_provider,
            "serviceTier":null,"effort":settings.effort,"summary":null,"personality":null,
            "collaborationMode":{"mode":"default","settings":{
                "model":model,"reasoning_effort":settings.effort,"developer_instructions":null
            }}
        }
    }})
}
