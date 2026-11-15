use super::{
    AutoPermissionMode, AutoRealization, PermissionModePlan, PermissionModeReason,
    PermissionModeRefusal, RequestedPermissionMode, plan_permission_mode,
};

const CLAUDE: AutoPermissionMode = AutoPermissionMode {
    realized_by: Some("auto"),
    fallback: Some("acceptEdits"),
    realization: AutoRealization::ClassifierEscalates,
    stand_in_reason: None,
};
const CURSOR: AutoPermissionMode = AutoPermissionMode {
    realized_by: Some("agent"),
    fallback: None,
    realization: AutoRealization::AsksFirst,
    stand_in_reason: None,
};

fn modes(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn set(
    requested: RequestedPermissionMode,
    mode: &str,
    accepted_fallback: Option<&str>,
    reason: Option<PermissionModeReason>,
) -> Result<PermissionModePlan, PermissionModeRefusal> {
    Ok(PermissionModePlan::Set {
        requested,
        mode: mode.to_owned(),
        accepted_fallback: accepted_fallback.map(str::to_owned),
        reason,
    })
}

#[test]
fn requested_mode_maps_to_the_agents_realization_or_is_refused_with_the_accepted_list() {
    let claude_modes = modes(&["default", "acceptEdits", "plan", "auto"]);
    let cursor_modes = modes(&["agent", "plan", "ask"]);
    let unspecified = RequestedPermissionMode::Unspecified;
    let auto = RequestedPermissionMode::Auto;
    let named = |name: &str| RequestedPermissionMode::AgentMode(name.to_owned());
    let cases = [
        // R3: no mode means auto.
        (
            "claude, no mode",
            unspecified.clone(),
            &claude_modes,
            CLAUDE,
            set(unspecified.clone(), "auto", Some("acceptEdits"), None),
        ),
        (
            "claude, auto",
            auto.clone(),
            &claude_modes,
            CLAUDE,
            set(auto.clone(), "auto", Some("acceptEdits"), None),
        ),
        (
            "claude, own name",
            named("plan"),
            &claude_modes,
            CLAUDE,
            set(named("plan"), "plan", None, None),
        ),
        (
            "claude without auto on offer falls back and says why",
            unspecified.clone(),
            &modes(&["default", "acceptEdits"]),
            CLAUDE,
            set(
                unspecified.clone(),
                "acceptEdits",
                None,
                Some(PermissionModeReason::AutoNotOffered),
            ),
        ),
        (
            "a stand-in realization says why",
            unspecified.clone(),
            &claude_modes,
            AutoPermissionMode {
                realized_by: Some("acceptEdits"),
                fallback: None,
                realization: AutoRealization::AsksFirst,
                stand_in_reason: Some(PermissionModeReason::EscalationUnproven),
            },
            set(
                unspecified.clone(),
                "acceptEdits",
                None,
                Some(PermissionModeReason::EscalationUnproven),
            ),
        ),
        (
            "cursor, no mode",
            unspecified.clone(),
            &cursor_modes,
            CURSOR,
            set(unspecified.clone(), "agent", None, None),
        ),
        (
            "cursor, auto",
            auto.clone(),
            &cursor_modes,
            CURSOR,
            set(auto.clone(), "agent", None, None),
        ),
        (
            "cursor, own name",
            named("ask"),
            &cursor_modes,
            CURSOR,
            set(named("ask"), "ask", None, None),
        ),
        (
            "unrealized agent keeps its mode and says why",
            auto.clone(),
            &modes(&["ask"]),
            AutoPermissionMode::UNREALIZED,
            Ok(PermissionModePlan::Keep {
                requested: auto,
                reason: Some(PermissionModeReason::AutoNotRealized),
            }),
        ),
        (
            "agent with no modes keeps what it has",
            unspecified.clone(),
            &modes(&[]),
            CURSOR,
            Ok(PermissionModePlan::Keep {
                requested: unspecified,
                reason: Some(PermissionModeReason::AutoNotOffered),
            }),
        ),
        // R5: anything else is refused with `auto` and the agent's own names.
        (
            "claude, bogus",
            named("yolo"),
            &claude_modes,
            CLAUDE,
            Err(PermissionModeRefusal {
                value: "yolo".to_owned(),
                accepted: modes(&["auto", "default", "acceptEdits", "plan"]),
            }),
        ),
        (
            "cursor, another agent's name",
            named("acceptEdits"),
            &cursor_modes,
            CURSOR,
            Err(PermissionModeRefusal {
                value: "acceptEdits".to_owned(),
                accepted: modes(&["auto", "agent", "plan", "ask"]),
            }),
        ),
        (
            "case differs",
            named("Auto"),
            &claude_modes,
            CLAUDE,
            Err(PermissionModeRefusal {
                value: "Auto".to_owned(),
                accepted: modes(&["auto", "default", "acceptEdits", "plan"]),
            }),
        ),
    ];
    for (name, requested, advertised, auto_mode, expected) in cases {
        assert_eq!(
            plan_permission_mode(requested, advertised, auto_mode),
            expected,
            "{name}"
        );
    }
}

#[test]
fn request_names_parse_into_closed_variants() {
    assert_eq!(
        RequestedPermissionMode::from_request(None),
        RequestedPermissionMode::Unspecified
    );
    assert_eq!(
        RequestedPermissionMode::from_request(Some("auto")),
        RequestedPermissionMode::Auto
    );
    assert_eq!(
        RequestedPermissionMode::from_request(Some("plan")),
        RequestedPermissionMode::AgentMode("plan".to_owned())
    );
}
