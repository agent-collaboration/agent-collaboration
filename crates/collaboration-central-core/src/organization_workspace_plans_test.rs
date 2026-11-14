//! Retained ledger text is validated against the admitted create before replay.
use super::retained_result;
use crate::{AdminRefusal, OrganizationAdminResult, person_request::CreateWorkspaceRequest};
use central_storage::{CorruptReason, StoreFailure};
use serde_json::{Value, json};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000da01";
const OPERATION: &str = "019f0000-0000-7000-8000-00000000da02";
const OTHER_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000da03";

fn request() -> CreateWorkspaceRequest {
    CreateWorkspaceRequest {
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        operation_id: OPERATION.to_owned().try_into().unwrap(),
        name: "Research".to_owned().try_into().unwrap(),
        description: "Shared research notes".to_owned(),
    }
}

fn applied() -> Value {
    json!({
        "result": "applied",
        "workspace": {
            "workspaceId": OPERATION,
            "name": "Research",
            "description": "Shared research notes",
            "state": "active"
        }
    })
}

fn assert_corrupt(encoded: &str) {
    assert_eq!(
        retained_result(encoded, &request()),
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: "request_log.answer".to_owned(),
            },
        })
    );
}

#[test]
fn retained_malformed_json_and_non_object_values_are_corrupt() {
    for encoded in ["", "{", "{\"result\":", "null", "[]", "42", "\"applied\""] {
        assert_corrupt(encoded);
    }
}

#[test]
fn retained_results_from_other_operations_and_unknown_results_are_corrupt() {
    for encoded in [
        json!({"result": "found", "workspace": applied()["workspace"]}),
        json!({"result": "records", "records": []}),
        json!({"result": "created", "workspace": applied()["workspace"]}),
        json!({"result": null, "workspace": applied()["workspace"]}),
        json!({"result": 1, "workspace": applied()["workspace"]}),
    ] {
        assert_corrupt(&encoded.to_string());
    }
}

#[test]
fn retained_refusals_accept_only_name_in_use() {
    for reason in [
        json!("notPermitted"),
        json!("secondFactorRequired"),
        json!("refreshRequired"),
        json!("authorizationSyncRequired"),
        json!("expired"),
        json!("requestTime"),
        json!("requestReused"),
        json!("conflict"),
        json!("rejected"),
        json!("unknown"),
        json!({"kind": "nameInUse"}),
        Value::Null,
        json!(false),
    ] {
        assert_corrupt(&json!({"result": "refused", "reason": reason}).to_string());
    }
    assert_corrupt(r#"{"result":"refused"}"#);
}

#[test]
fn retained_applied_workspace_must_match_the_request_and_active_state() {
    for (field, value) in [
        ("workspaceId", json!(OTHER_WORKSPACE)),
        ("workspaceId", json!("not-a-workspace-id")),
        ("name", json!("Other name")),
        ("description", json!("Other description")),
        ("state", json!("archived")),
        ("state", json!("archiving")),
    ] {
        let mut retained = applied();
        retained["workspace"][field] = value;
        assert_corrupt(&retained.to_string());
    }
}

#[test]
fn retained_workspace_name_is_never_trimmed_into_a_matching_name() {
    for name in [" Research", "Research ", " Research ", "\tResearch\n"] {
        let mut retained = applied();
        retained["workspace"]["name"] = json!(name);
        assert_corrupt(&retained.to_string());
    }
}

#[test]
fn retained_applied_requires_every_known_field_with_its_exact_type() {
    for field in ["result", "workspace"] {
        let mut retained = applied();
        retained.as_object_mut().unwrap().remove(field);
        assert_corrupt(&retained.to_string());
    }
    for field in ["workspaceId", "name", "description", "state"] {
        let mut retained = applied();
        retained["workspace"].as_object_mut().unwrap().remove(field);
        assert_corrupt(&retained.to_string());
        for value in [Value::Null, json!(12), json!(false), json!([]), json!({})] {
            let mut retained = applied();
            retained["workspace"][field] = value;
            assert_corrupt(&retained.to_string());
        }
    }
    for value in [
        Value::Null,
        json!(12),
        json!(false),
        json!([]),
        json!("workspace"),
    ] {
        let mut retained = applied();
        retained["workspace"] = value;
        assert_corrupt(&retained.to_string());
    }
}

#[test]
fn valid_retained_applied_and_name_in_use_answers_are_accepted() {
    let expected: OrganizationAdminResult = serde_json::from_value(applied()).unwrap();
    assert_eq!(
        retained_result(&applied().to_string(), &request()).unwrap(),
        expected
    );
    let reordered = format!(
        r#" {{
            "workspace": {{
                "state": "active", "description": "Shared research notes",
                "name": "Research", "workspaceId": "{OPERATION}"
            }},
            "result": "applied"
        }} "#
    );
    assert_eq!(retained_result(&reordered, &request()).unwrap(), expected);
    assert_eq!(
        retained_result(
            " { \"reason\" : \"nameInUse\", \"result\" : \"refused\" } ",
            &request()
        )
        .unwrap(),
        OrganizationAdminResult::Refused {
            reason: AdminRefusal::NameInUse
        }
    );
}

#[test]
fn unknown_retained_top_level_fields_are_corrupt() {
    let mut retained = applied();
    retained["futureEnvelope"] =
        json!({"version": 2, "items": [null, true, {"result": "unknown"}]});
    assert_corrupt(&retained.to_string());
    assert_corrupt(r#"{"result":"refused","reason":"nameInUse","future":{"opaque":true}}"#);
}

#[test]
fn unknown_retained_workspace_fields_are_corrupt() {
    let mut retained = applied();
    retained["workspace"]["futureView"] = json!({"nested": ["opaque", {"state": "unknown"}]});
    assert_corrupt(&retained.to_string());
    retained["futureEnvelope"] = json!(true);
    assert_corrupt(&retained.to_string());
    retained["workspace"]["state"] = json!("archiving");
    assert_corrupt(&retained.to_string());
}
