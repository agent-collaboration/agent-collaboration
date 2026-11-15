//! The actual permission mode a Codex session runs in, from native's own report
//! of the thread it started (R3).
//!
//! Router auto for Codex is workspace-write with the Router as approvals
//! reviewer and the system temporary folders withheld. It is reported as
//! `auto` only when native read back exactly that; otherwise the access the
//! session runs under is reported, with the reason.

use collaboration_protocol::{
    PermissionDifference, ProviderModeReason, RouterAccess, SettingsObservation,
};
use serde::Serialize;

/// The Router-level mode name every agent accepts.
const ROUTER_AUTO: &str = "auto";

/// `_meta.codexRouter.permissionMode` on a create result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexPermissionMode {
    pub(crate) actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<ProviderModeReason>,
}

pub(crate) fn codex_permission_mode(
    access: RouterAccess,
    observation: &SettingsObservation,
    differences: &[PermissionDifference],
) -> CodexPermissionMode {
    match access {
        // An explicit narrower access, not auto's realization.
        RouterAccess::WriteRestricted => CodexPermissionMode {
            actual: super::access_name(access).to_owned(),
            reason: Some(ProviderModeReason::AutoNotRealized),
        },
        RouterAccess::WorkspaceWrite => {
            let reviewer_is_router = matches!(
                observation,
                SettingsObservation::Observed { approvals_reviewer, .. }
                    if approvals_reviewer.as_ref().as_ref().and_then(serde_json::Value::as_str)
                        == Some("user")
            );
            if reviewer_is_router && differences.is_empty() {
                CodexPermissionMode {
                    actual: ROUTER_AUTO.to_owned(),
                    reason: None,
                }
            } else {
                CodexPermissionMode {
                    actual: super::access_name(access).to_owned(),
                    reason: Some(ProviderModeReason::AutoNotApplied),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "codex_permission_mode_test.rs"]
mod tests;
