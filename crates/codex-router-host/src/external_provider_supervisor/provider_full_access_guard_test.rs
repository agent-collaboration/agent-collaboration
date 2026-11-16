use super::is_full_access_mode;
use collaboration_protocol::ProviderKind;

/// Oracle: D1 R19, "Claude's bypassPermissions ... refused"; every other
/// Claude mode and every Cursor mode stays allowed (R5).
#[test]
fn only_claudes_bypass_permissions_is_full_access() {
    // Arrange
    let cases = [
        (ProviderKind::ClaudeCode, "bypassPermissions", true),
        (ProviderKind::ClaudeCode, "auto", false),
        (ProviderKind::ClaudeCode, "acceptEdits", false),
        (ProviderKind::ClaudeCode, "default", false),
        (ProviderKind::ClaudeCode, "plan", false),
        (ProviderKind::ClaudeCode, "dontAsk", false),
        (ProviderKind::Cursor, "agent", false),
        (ProviderKind::Cursor, "ask", false),
        (ProviderKind::Cursor, "bypassPermissions", false),
    ];

    for (provider, mode, expected) in cases {
        // Act
        let refused = is_full_access_mode(provider, mode);

        // Assert
        assert_eq!(refused, expected, "{provider:?} {mode}");
    }
}
