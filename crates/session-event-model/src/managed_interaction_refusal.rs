//! Immediate refusal responses for Router-managed provider interactions.

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedInteractionRefusal {
    ApprovalsUnavailableOnBuild,
}

impl ManagedInteractionRefusal {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ApprovalsUnavailableOnBuild => "approvalsUnavailableOnBuild",
        }
    }

    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ApprovalsUnavailableOnBuild => "approvals are not available on this build",
        }
    }

    /// The Codex 0.166.0 ServerRequest catalogue, including experimental requests.
    #[must_use]
    pub fn codex_response(self, method: &str) -> Option<Value> {
        match method {
            "item/commandExecution/requestApproval" => Some(self.codex_command()),
            "item/fileChange/requestApproval" => Some(self.codex_file_change()),
            "item/permissions/requestApproval" => Some(self.codex_permissions()),
            "item/tool/requestUserInput" => Some(self.codex_user_input()),
            "mcpServer/elicitation/request" => Some(self.codex_mcp_elicitation()),
            // The managed v2 app-server emits the item-scoped approvals above.
            "applyPatchApproval" | "execCommandApproval" => None,
            // These requests require capabilities outside managed prompting.
            "item/tool/call"
            | "account/chatgptAuthTokens/refresh"
            | "attestation/generate"
            | "currentTime/read" => None,
            _ => None, // Unrecognized method, rejected by the pinned schema.
        }
    }

    /// Requests owned by the managed ACP refusal path. The connection guard
    /// claims these before any handler can answer or attach them to a prompt.
    #[must_use]
    pub fn owns_acp_callback(method: &str) -> bool {
        match method {
            "session/request_permission"
            | "cursor/ask_question"
            | "elicitation/create"
            | "cursor/create_plan" => true,
            // Cursor activities have a separate acknowledgement handler.
            "cursor/update_todos" => false,
            // The managed client advertises none of these capabilities.
            "fs/read_text_file"
            | "fs/write_text_file"
            | "terminal/create"
            | "terminal/output"
            | "terminal/release"
            | "terminal/wait_for_exit"
            | "terminal/kill"
            | "mcp/connect"
            | "mcp/message"
            | "mcp/disconnect" => false,
            _ => false, // Other extensions reach method-not-found.
        }
    }

    #[must_use]
    pub fn codex_command(self) -> Value {
        json!({"decision":"cancel"})
    }

    #[must_use]
    pub fn codex_file_change(self) -> Value {
        json!({"decision":"cancel"})
    }

    #[must_use]
    pub fn codex_permissions(self) -> Value {
        json!({"permissions":{},"scope":"turn"})
    }

    #[must_use]
    pub fn codex_user_input(self) -> Value {
        json!({"answers":{}})
    }

    #[must_use]
    pub fn codex_mcp_elicitation(self) -> Value {
        json!({"action":"cancel"})
    }

    #[must_use]
    pub fn acp_permission(self) -> Value {
        json!({"outcome":{"outcome":"cancelled"}})
    }

    #[must_use]
    pub fn acp_question(self) -> Value {
        json!({"outcome":{"outcome":"cancelled"}})
    }

    #[must_use]
    pub fn acp_form(self) -> Value {
        json!({"action":"cancel"})
    }

    #[must_use]
    pub fn acp_cursor_plan(self) -> Value {
        json!({"outcome":"cancelled"})
    }
}
