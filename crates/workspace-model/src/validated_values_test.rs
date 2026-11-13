use super::{ChangePosition, PostText, ResourceName};
use crate::{ReferenceSource, WorkLogBody, WorkStatus};

#[test]
fn text_validation_ports_utf8_and_layout_policy() {
    assert_eq!(
        ResourceName::try_from("  area  ".to_owned()).map(|name| name.as_str().to_owned()),
        Ok("area".to_owned())
    );
    assert!(ResourceName::try_from("é".repeat(129)).is_err());
    assert!(PostText::try_from("line\n\tline".to_owned()).is_ok());
    for control in ['\0', '\u{0008}', '\u{000b}', '\u{001f}'] {
        assert!(PostText::try_from(format!("before{control}after")).is_err());
    }
    assert!(PostText::try_from("before\u{007f}after".to_owned()).is_ok());
    assert!(ChangePosition::try_from(9_007_199_254_740_992).is_err());
    assert!(serde_json::from_str::<crate::BacklogDescription>("\"\"").is_err());
    assert!(serde_json::from_str::<crate::BacklogDescription>("\"   \"").is_err());
}

#[test]
fn ported_search_query_trims_and_keeps_utf8_bounds() {
    let query = crate::SearchQuery::try_from(" \tneedle\n ".to_owned()).unwrap();
    assert_eq!(query.as_str(), "needle");
    assert!(crate::SearchQuery::try_from(" \t\n ".to_owned()).is_err());
    for value in ["n".repeat(256), "é".repeat(128)] {
        assert!(crate::SearchQuery::try_from(value).is_ok());
    }
    for value in ["n".repeat(257), "é".repeat(129)] {
        assert!(crate::SearchQuery::try_from(value).is_err());
    }
}

#[test]
fn r3_f_r20_b_unknown_roles_fail_in_commands_and_stored_assignment_rows() {
    use crate::{DeliverableRoleAssignment, ProjectRoleAssignment, RoleCommand};
    let id = "01890f2e-7b4c-7cc0-98c4-000000000001";
    let project_command = format!(
        r#"{{"kind":"grantProject","assignmentId":"{id}","projectId":"{id}","role":"owner","holder":"{id}"}}"#
    );
    let deliverable_command = format!(
        r#"{{"kind":"grantDeliverable","assignmentId":"{id}","deliverableId":"{id}","role":"owner","holder":"{id}"}}"#
    );
    for (command, allowed) in [
        (project_command, vec!["lead", "advisor"]),
        (
            deliverable_command,
            vec!["implementer", "verifier", "reviewer"],
        ),
    ] {
        let error = serde_json::from_str::<RoleCommand>(&command)
            .unwrap_err()
            .to_string();
        for role in allowed {
            assert!(error.contains(role));
        }
    }
    let project_row = format!(
        r#"{{"projectRoleId":"{id}","projectId":"{id}","role":"owner","holder":"{id}","grantedAt":"2026-11-13T12:00:00Z","state":{{"kind":"active"}}}}"#
    );
    let error = serde_json::from_str::<ProjectRoleAssignment>(&project_row)
        .unwrap_err()
        .to_string();
    assert!(error.contains("owner") && error.contains("lead") && error.contains("advisor"));
    let deliverable_row = format!(
        r#"{{"deliverableRoleId":"{id}","deliverableId":"{id}","role":"owner","holder":"{id}","grantedAt":"2026-11-13T12:00:00Z","state":{{"kind":"active"}}}}"#
    );
    let error = serde_json::from_str::<DeliverableRoleAssignment>(&deliverable_row)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("owner")
            && error.contains("implementer")
            && error.contains("verifier")
            && error.contains("reviewer")
    );
}
#[test]
fn closed_states_and_sources_reject_unknown_or_invalid_shapes() {
    assert!(serde_json::from_str::<WorkStatus>("\"done\"").is_err());
    assert!(
        serde_json::from_str::<ReferenceSource>(
            r#"{"kind":"issue","id":"01890f2e-7b4c-7cc0-98c4-000000000001"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<WorkLogBody>(r#"{"kind":"redacted","body":"hidden leak"}"#).is_err()
    );
}

#[test]
fn r3_f_r20_b_roles_are_closed_and_wrong_scope_text_names_allowed_roles() {
    use crate::{DeliverableRole, ProjectRole, RoleName};
    let project_error = serde_json::from_str::<ProjectRole>("\"implementer\"")
        .unwrap_err()
        .to_string();
    assert!(project_error.contains("lead") && project_error.contains("advisor"));
    let deliverable_error = serde_json::from_str::<DeliverableRole>("\"lead\"")
        .unwrap_err()
        .to_string();
    assert!(
        deliverable_error.contains("implementer")
            && deliverable_error.contains("verifier")
            && deliverable_error.contains("reviewer")
    );
    assert!(serde_json::from_str::<RoleName>(r#"{"kind":"project","role":"reviewer"}"#).is_err());
    assert!(
        serde_json::from_str::<RoleName>(r#"{"kind":"deliverable","role":"reviewer"}"#).is_ok()
    );
}
