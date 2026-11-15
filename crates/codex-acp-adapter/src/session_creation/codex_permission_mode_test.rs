use super::codex_permission_mode;
use collaboration_protocol::{
    CodexPermissionModeReport, PermissionDifference, ProviderModeReason, RouterAccess,
    SettingsObservation, SettingsObservationSource, SettingsUnavailableReason,
};
use serde_json::{Value, json};

/// Native's read-back of a started thread, field by field.
fn observed(
    profile: &str,
    sandbox: &str,
    policy: Option<&str>,
    reviewer: &str,
) -> SettingsObservation {
    SettingsObservation::Observed {
        source: SettingsObservationSource::ThreadStart,
        observed_at: "2026-11-14T00:00:00Z".to_owned(),
        router_access: Some(RouterAccess::WorkspaceWrite),
        native_sandbox: Some(json!({"type":sandbox,"writableRoots":[]})),
        permission_profile: Some(json!({"id":profile,"extends":":workspace"})),
        approval_policy: Box::new(policy.map(Value::from)),
        approvals_reviewer: Box::new(Some(json!(reviewer))),
    }
}

fn workspace(sandbox: &str, policy: Option<&str>, reviewer: &str) -> SettingsObservation {
    observed("router-workspace-write", sandbox, policy, reviewer)
}

fn mode(
    access: RouterAccess,
    actual: Option<&str>,
    reason: Option<ProviderModeReason>,
) -> CodexPermissionModeReport {
    CodexPermissionModeReport {
        access,
        requested_mode: Some("auto".to_owned()),
        actual: actual.map(str::to_owned),
        reason,
    }
}

/// Oracle: R3 for Codex — the actual mode comes only from native's read-back:
/// auto only when native reported the Router profile, a workspace-write
/// sandbox, on-request approvals and the Router as reviewer with no
/// difference; a missing or contradicting report claims no actual mode.
#[test]
fn codex_reports_only_the_mode_native_read_back() {
    use ProviderModeReason::{AutoNotApplied, AutoNotRealized};
    use RouterAccess::{WorkspaceWrite, WriteRestricted};
    let cases = [
        (
            "realized",
            WorkspaceWrite,
            workspace("workspaceWrite", Some("on-request"), "user"),
            vec![],
            mode(WorkspaceWrite, Some("auto"), None),
        ),
        (
            "automatic reviewer",
            WorkspaceWrite,
            workspace("workspaceWrite", Some("on-request"), "auto_review"),
            vec![],
            mode(
                WorkspaceWrite,
                Some("workspace-write"),
                Some(AutoNotApplied),
            ),
        ),
        (
            "approval policy missing",
            WorkspaceWrite,
            workspace("workspaceWrite", None, "user"),
            vec![],
            mode(
                WorkspaceWrite,
                Some("workspace-write"),
                Some(AutoNotApplied),
            ),
        ),
        (
            "approval policy different",
            WorkspaceWrite,
            workspace("workspaceWrite", Some("never"), "user"),
            vec![],
            mode(
                WorkspaceWrite,
                Some("workspace-write"),
                Some(AutoNotApplied),
            ),
        ),
        (
            "a permission difference",
            WorkspaceWrite,
            workspace("workspaceWrite", Some("on-request"), "user"),
            vec![PermissionDifference::Sandbox],
            mode(
                WorkspaceWrite,
                Some("workspace-write"),
                Some(AutoNotApplied),
            ),
        ),
        (
            "contradicting sandbox",
            WorkspaceWrite,
            workspace("dangerFullAccess", Some("on-request"), "user"),
            vec![],
            mode(WorkspaceWrite, None, Some(AutoNotApplied)),
        ),
        (
            "contradicting profile",
            WorkspaceWrite,
            observed(
                "router-write-restricted",
                "workspaceWrite",
                Some("on-request"),
                "user",
            ),
            vec![],
            mode(WorkspaceWrite, None, Some(AutoNotApplied)),
        ),
        (
            "settings missing",
            WorkspaceWrite,
            SettingsObservation::Unavailable {
                reason: SettingsUnavailableReason::NativeResponseOmittedSettings,
            },
            vec![],
            mode(WorkspaceWrite, None, Some(AutoNotApplied)),
        ),
        (
            "write-restricted read back",
            WriteRestricted,
            observed(
                "router-write-restricted",
                "workspaceWrite",
                Some("on-request"),
                "user",
            ),
            vec![],
            mode(
                WriteRestricted,
                Some("write-restricted"),
                Some(AutoNotRealized),
            ),
        ),
        (
            "write-restricted missing",
            WriteRestricted,
            SettingsObservation::Unavailable {
                reason: SettingsUnavailableReason::NativeResponseOmittedSettings,
            },
            vec![],
            mode(WriteRestricted, None, Some(AutoNotRealized)),
        ),
    ];
    for (name, access, observation, differences, expected) in cases {
        assert_eq!(
            codex_permission_mode(access, Some("auto"), &observation, &differences),
            expected,
            "{name}"
        );
    }
    assert_eq!(
        serde_json::to_value(mode(WorkspaceWrite, None, Some(AutoNotApplied))).unwrap(),
        json!({"access":"workspace-write","requestedMode":"auto","reason":"autoNotApplied"})
    );
}
