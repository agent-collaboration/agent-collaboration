//! The closed provisioning payload: F1c's organization plus B2's signed stamp.
use super::contract_artifacts;
use collaboration_central_core::identity_contract::{
    ProvisionOrganization, ProvisionOrganizationResult,
};
use serde_json::json;

// Oracle: the B2 doc's flow (ProvisionOrganization { organizationId,
// organizationStatement }) as the Lead adopted it over F1c's organization-only body.
// Catch an invented person, a retired operation, a missing stamp or an unknown
// result being accepted.
#[test]
fn provision_organization_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganization>(
        "ProvisionOrganization",
        vec![json!({
            "organizationId":"019a3dce-2000-7000-8000-000000000001",
            "organizationStatement":"eyJ0eXAiOiJmbGVldCJ9.eyJzdWIiOiJvcmcifQ.c2lnbmF0dXJl"
        })],
        false,
    );
}

// The F1c organization-only body is now refused by both serde and the schema.
#[test]
fn provision_organization_requires_the_signed_statement() {
    let schema = serde_json::to_value(schemars::schema_for!(ProvisionOrganization)).unwrap();
    let validator = crate::schema_validation::contract_validator(&schema).unwrap();
    for body in [
        json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"}),
        json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","organizationStatement":""}),
        json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","organizationStatement":"a.b"}),
    ] {
        assert!(
            serde_json::from_value::<ProvisionOrganization>(body.clone()).is_err(),
            "{body}"
        );
        assert!(!validator.is_valid(&body), "{body}");
    }
}

#[test]
fn provision_organization_result_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganizationResult>(
        "ProvisionOrganizationResult",
        vec![json!({"result":"provisioned"})],
        false,
    );
}

// Keep the old literal inputs as negatives after removing the retired command types.
// Better Auth owns these changes; accepting them at central would recreate that owner.
#[test]
fn retired_people_commands_are_not_provisioning_requests() {
    let retired: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        serde_json::from_str(include_str!("../fixtures/retired_people_commands.json")).unwrap();
    let schema = serde_json::to_value(schemars::schema_for!(ProvisionOrganization)).unwrap();
    let validator = crate::schema_validation::contract_validator(&schema).unwrap();
    assert_eq!(retired.len(), 4, "keep every retired people-command oracle");

    for (command, requests) in retired {
        assert!(
            !requests.is_empty(),
            "retired {command} has no literal input"
        );
        for request in requests {
            assert!(
                serde_json::from_value::<ProvisionOrganization>(request.clone()).is_err(),
                "retired {command} became a provisioning request"
            );
            assert!(
                !validator.is_valid(&request),
                "provisioning schema accepted retired {command}"
            );
        }
    }
}
