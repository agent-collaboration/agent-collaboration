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
    assert!(PostText::try_from("text\0".to_owned()).is_err());
    assert!(ChangePosition::try_from(9_007_199_254_740_992).is_err());
    assert!(serde_json::from_str::<crate::BacklogDescription>("\"\"").is_err());
    assert!(serde_json::from_str::<crate::BacklogDescription>("\"   \"").is_err());
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
