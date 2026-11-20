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
