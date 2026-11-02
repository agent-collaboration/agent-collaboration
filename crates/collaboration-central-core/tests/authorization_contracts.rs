//! Authorization wire oracles from the reviewed permission and provisioning rules.
use collaboration_central_core::identity_contract::{
    DecideDeviceLogin, FleetPermission, ProvisionOrganization, ProvisionOrganizationResult,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;
#[cfg(test)]
use contract_artifacts::schema_validation;

// PD 2530c3a3's complete route table, independent of the Rust enum's variants.
const ROUTE_PERMISSIONS: [&str; 9] = [
    "workspace:create",
    "workspaceAdmin:allowMachine",
    "workspaceAdmin:withdrawMachine",
    "workspaceAdmin:removeSession",
    "machine:enroll",
    "machine:revoke",
    "machine:setTokenLifetime",
    "organization:view",
    "organization:readAudit",
];

// Wrong behavior: adding another permission, serializing an object, or changing
// one resource/action's spelling. Both the serializer and schema stay real.
#[test]
fn fleet_permission_wire_contract() {
    // Arrange
    let schema = serde_json::to_value(schemars::schema_for!(FleetPermission)).unwrap();
    let mut expected = ROUTE_PERMISSIONS.to_vec();
    expected.sort_unstable();

    // Act
    let mut published = schema["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    published.sort_unstable();

    // Assert
    assert_eq!(schema["type"], "string");
    assert_eq!(published, expected);
    contract_artifacts::assert_contract::<FleetPermission>(
        "FleetPermission",
        ROUTE_PERMISSIONS
            .iter()
            .map(|permission| json!(permission))
            .collect(),
        false,
    );
}

// This contract represents what was checked. Requiring machine:enroll is the
// authority's policy, so a structurally valid different permission must parse.
#[test]
fn checked_device_permission_accepts_each_route_permission() {
    // Arrange
    let request = json!({
        "organizationId": "019a3dce-2000-7000-8000-000000000001",
        "approver": "019a3dce-2000-7000-8000-000000000001",
        "requestId": "019a3dce-2000-7000-8000-000000000002",
        "installation": "019a3dce-2000-7000-8000-000000000001",
        "peer": "019a3dce-2000-7000-8000-000000000002",
        "label": "fixture machine",
        "allowedWorkspaces": []
    });
    let schema = serde_json::to_value(schemars::schema_for!(DecideDeviceLogin)).unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();

    for permission in ROUTE_PERMISSIONS {
        let mut input = request.clone();
        input["permissionChecked"] = json!(permission);

        // Act
        let parsed: DecideDeviceLogin = serde_json::from_value(input.clone()).unwrap();
        let encoded = serde_json::to_value(parsed).unwrap();

        // Assert
        assert_eq!(encoded, input);
        assert!(validator.is_valid(&input));
    }
}

// Provisioning carries a Better Auth organization's ID, with no owner payload.
#[test]
fn provision_organization_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganization>(
        "ProvisionOrganization",
        vec![json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

// Lead 66cb305d defines one answer. Failure to receive it remains unknown at
// the call boundary; a fabricated refused answer must not parse as a result.
#[test]
fn provision_organization_result_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganizationResult>(
        "ProvisionOrganizationResult",
        vec![json!({"result":"provisioned"})],
        false,
    );
}
