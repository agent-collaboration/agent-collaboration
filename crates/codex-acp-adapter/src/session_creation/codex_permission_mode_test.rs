use super::{CodexPermissionMode, codex_permission_mode};
use collaboration_protocol::{
    PermissionDifference, ProviderModeReason, RouterAccess, SettingsObservation,
    SettingsObservationSource, SettingsUnavailableReason,
};
use serde_json::json;

fn observed(reviewer: &str) -> SettingsObservation {
    SettingsObservation::Observed {
        source: SettingsObservationSource::ThreadStart,
        observed_at: "2026-11-14T00:00:00Z".to_owned(),
        router_access: Some(RouterAccess::WorkspaceWrite),
        native_sandbox: None,
        permission_profile: None,
        approval_policy: Box::new(Some(json!("on-request"))),
        approvals_reviewer: Box::new(Some(json!(reviewer))),
    }
}

fn mode(actual: &str, reason: Option<ProviderModeReason>) -> CodexPermissionMode {
    CodexPermissionMode {
        actual: actual.to_owned(),
        reason,
    }
}

/// Oracle: R3 for Codex — auto is reported only when native read back the
/// Router as reviewer and no permission difference; otherwise the access is
/// reported with why it is not auto.
#[test]
fn codex_reports_auto_only_when_native_read_back_its_realization() {
    let cases = [
        (
            RouterAccess::WorkspaceWrite,
            observed("user"),
            vec![],
            mode("auto", None),
        ),
        (
            RouterAccess::WorkspaceWrite,
            observed("auto_review"),
            vec![],
            mode("workspace-write", Some(ProviderModeReason::AutoNotApplied)),
        ),
        (
            RouterAccess::WorkspaceWrite,
            observed("user"),
            vec![PermissionDifference::Sandbox],
            mode("workspace-write", Some(ProviderModeReason::AutoNotApplied)),
        ),
        (
            RouterAccess::WorkspaceWrite,
            SettingsObservation::Unavailable {
                reason: SettingsUnavailableReason::NativeResponseOmittedSettings,
            },
            vec![],
            mode("workspace-write", Some(ProviderModeReason::AutoNotApplied)),
        ),
        (
            RouterAccess::WriteRestricted,
            observed("user"),
            vec![],
            mode(
                "write-restricted",
                Some(ProviderModeReason::AutoNotRealized),
            ),
        ),
    ];
    for (access, observation, differences, expected) in cases {
        assert_eq!(
            codex_permission_mode(access, &observation, &differences),
            expected,
            "{access:?} {observation:?} {differences:?}"
        );
    }
    assert_eq!(
        serde_json::to_value(mode(
            "workspace-write",
            Some(ProviderModeReason::AutoNotApplied)
        ))
        .unwrap(),
        json!({"actual":"workspace-write","reason":"autoNotApplied"})
    );
}
