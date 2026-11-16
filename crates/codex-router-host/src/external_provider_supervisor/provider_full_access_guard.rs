//! Full access is refused until session authentication exists (D1 R19). A
//! start or a mode change into a mode that switches every check off is
//! refused before anything reaches the agent; sessions already running are
//! not touched.

use collaboration_protocol::ProviderKind;

/// Why a full-access mode is refused, as every caller is told.
pub(super) const FULL_ACCESS_REFUSAL: &str = "full access is refused until the Router can prove the request comes from you (session authentication, Feature 2)";

/// Whether `mode` is the provider's mode that switches every check off.
/// Claude advertises bypassPermissions. Cursor's ACP modes (agent, plan, ask)
/// include none: its run-everything is a CLI flag, not a mode a Router
/// session can name. Codex sessions take only the Router's two access modes.
pub(super) fn is_full_access_mode(provider: &ProviderKind, mode: &str) -> bool {
    match provider {
        ProviderKind::ClaudeCode => mode == "bypassPermissions",
        ProviderKind::Cursor => false,
    }
}

#[cfg(test)]
#[path = "provider_full_access_guard_test.rs"]
mod tests;
