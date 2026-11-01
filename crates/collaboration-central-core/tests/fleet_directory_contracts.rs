//! The directory contracts follow the one-central-per-organization wire oracle.
use collaboration_central_core::identity_contract::CreateOrganization;
use serde_json::{Value, json};

fn central_record_literal() -> Value {
    json!({
        "addresses": {
            "sessionEndpoint": "https://central-a.example.invalid/v1/session",
            "machineAddress": "https://central-a.example.invalid/machine",
            "adminAddress": "https://central-a.example.invalid/admin",
            "admissionAddress": "https://central-a.example.invalid/admission",
            "standingAddress": "https://central-a.example.invalid/standing",
            "machineTokenAddress": "https://central-a.example.invalid/machine-token",
            "receiver": "019a3dce-2000-7000-8000-000000000002",
            "relay": "019a3dce-2000-7000-8000-000000000003"
        },
        "directoryCalls": {
            "approvalAuthorizeAddress": "https://central-a.example.invalid/approval/authorize",
            "approvalActivateAddress": "https://central-a.example.invalid/approval/activate",
            "approvalCancelAddress": "https://central-a.example.invalid/approval/cancel",
            "initializationAddress": "https://central-a.example.invalid/initialize"
        }
    })
}

// Wrong behavior: the operator cannot supply the central that must initialize
// this organization. Oracle: Program Design "The new requests, exactly" and
// the by-hand runbook's step 3. This keeps the real serde boundary.
#[test]
fn fleet_directory_creation_carries_the_whole_central_record() {
    // Arrange
    let request = json!({
        "organizationId": "019a3dce-2000-7000-8000-000000000001",
        "ownerVerifiedEmail": "owner@example.invalid",
        "central": central_record_literal()
    });

    // Act
    let parsed = serde_json::from_value::<CreateOrganization>(request.clone());

    // Assert
    let parsed = parsed.expect("the operator supplies the whole CentralRecord");
    assert_eq!(serde_json::to_value(parsed).unwrap(), request);
    let mut missing_central = request;
    missing_central.as_object_mut().unwrap().remove("central");
    assert!(serde_json::from_value::<CreateOrganization>(missing_central).is_err());
}
