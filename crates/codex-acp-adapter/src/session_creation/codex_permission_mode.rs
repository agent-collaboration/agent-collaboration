//! The actual permission mode a Codex session runs in, from native's own report
//! of the thread it started (R3).
//!
//! Router auto for Codex is workspace-write with on-request approvals, the
//! Router as approvals reviewer, and the system temporary folders withheld. The
//! actual mode is taken only from what native read back: the selected Router
//! profile and its workspace-write sandbox. When that report is missing or
//! contradicts the requested access, no actual mode is claimed.

use super::{access_name, router_permission_profile::profile_name};
use collaboration_protocol::{
    CodexPermissionModeReport, PermissionDifference, ProviderModeReason, RouterAccess,
    SettingsObservation,
};
use serde_json::Value;

/// The Router-level mode name every agent accepts.
const ROUTER_AUTO: &str = "auto";

pub(crate) fn codex_permission_mode(
    access: RouterAccess,
    requested_mode: Option<&str>,
    observation: &SettingsObservation,
    differences: &[PermissionDifference],
) -> CodexPermissionModeReport {
    // Write-restricted is an explicit narrower access, never auto's realization.
    let reason_when_not_auto = match access {
        RouterAccess::WriteRestricted => ProviderModeReason::AutoNotRealized,
        RouterAccess::WorkspaceWrite => ProviderModeReason::AutoNotApplied,
    };
    let report =
        |actual: Option<&str>, reason: Option<ProviderModeReason>| CodexPermissionModeReport {
            access,
            requested_mode: requested_mode.map(str::to_owned),
            actual: actual.map(str::to_owned),
            reason,
        };
    let SettingsObservation::Observed {
        native_sandbox,
        permission_profile,
        approval_policy,
        approvals_reviewer,
        ..
    } = observation
    else {
        return report(None, Some(reason_when_not_auto));
    };
    let text = |value: Option<&Value>, key: Option<&str>| {
        let value = value?;
        key.map_or(Some(value), |key| value.get(key))?
            .as_str()
            .map(str::to_owned)
    };
    let profile = text(permission_profile.as_ref(), Some("id"));
    let sandbox = text(native_sandbox.as_ref(), Some("type"));
    // Native must report the profile Router selected and its workspace-write
    // sandbox; anything else contradicts the requested access.
    if profile.as_deref() != Some(profile_name(access))
        || sandbox.as_deref() != Some("workspaceWrite")
    {
        return report(None, Some(reason_when_not_auto));
    }
    let asks_the_router = text(approval_policy.as_ref().as_ref(), None).as_deref()
        == Some("on-request")
        && text(approvals_reviewer.as_ref().as_ref(), None).as_deref() == Some("user");
    if access == RouterAccess::WorkspaceWrite && asks_the_router && differences.is_empty() {
        report(Some(ROUTER_AUTO), None)
    } else {
        report(Some(access_name(access)), Some(reason_when_not_auto))
    }
}

#[cfg(test)]
#[path = "codex_permission_mode_test.rs"]
mod tests;
