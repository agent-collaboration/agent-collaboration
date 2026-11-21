//! Literal wire oracles and serializer-generated artifact freshness.
#[cfg(test)]
use crate::contract_artifacts;
use collaboration_central_core::identity_contract::{
    AllowedWorkspaces, DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision,
    DeviceLoginRefusal, DisableMachineClient, DisabledMachineClient, EndedMachineClient,
    EnrollPermission, MachinePrincipal, MachineTokenClaims, MachineTokenDecision,
    MachineTokenRefusal, PresentedMachine,
};
use fleet_contracts::MachineConnectionRefusal;
use serde_json::json;

#[test]
fn decide_device_login_wire_contract() {
    contract_artifacts::assert_contract::<DecideDeviceLogin>(
        "DecideDeviceLogin",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","approver":"019a3dce-2000-7000-8000-000000000001","permissionChecked":"machine:enroll","requestId":"019a3dce-2000-7000-8000-000000000002","machineId":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine","allowedWorkspaces":["019a3dce-2000-7000-8000-000000000001"],"approverSecondFactorAt":null,"codeExpiresAt":1793448600_i64}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","approver":"019a3dce-2000-7000-8000-000000000001","permissionChecked":"machine:enroll","requestId":"019a3dce-2000-7000-8000-000000000002","machineId":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine","allowedWorkspaces":[],"approverSecondFactorAt":1793448000_i64,"codeExpiresAt":1793448600_i64}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","approver":"019a3dce-2000-7000-8000-000000000001","permissionChecked":"machine:enroll","requestId":"019a3dce-2000-7000-8000-000000000002","machineId":"019a3dce-2000-7000-8000-000000000001","label":"é".repeat(128),"allowedWorkspaces":vec!["019a3dce-2000-7000-8000-000000000001";100],"approverSecondFactorAt":0,"codeExpiresAt":1793448600_i64}),
        ],
        false,
    );
}

#[test]
fn allowed_workspaces_wire_contract() {
    contract_artifacts::assert_contract::<AllowedWorkspaces>(
        "AllowedWorkspaces",
        vec![
            json!([]),
            json!(["019a3dce-2000-7000-8000-000000000001"]),
            json!(vec!["019a3dce-2000-7000-8000-000000000001"; 100]),
        ],
        false,
    );
}

#[test]
fn enroll_permission_wire_contract() {
    contract_artifacts::assert_contract::<EnrollPermission>(
        "EnrollPermission",
        vec![json!("machine:enroll")],
        false,
    );
}

#[test]
fn ended_machine_client_wire_contract() {
    contract_artifacts::assert_contract::<EndedMachineClient>(
        "EndedMachineClient",
        vec![
            json!({"enrollmentId":"019a3dce-2000-7000-8000-000000000001","clientId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn device_login_decision_wire_contract() {
    contract_artifacts::assert_contract::<DeviceLoginDecision>(
        "DeviceLoginDecision",
        vec![
            json!({"result":"approved","enrollmentId":"019a3dce-2000-7000-8000-000000000002","label":"fixture machine","allowedWorkspaces":["019a3dce-2000-7000-8000-000000000001"]}),
            json!({"result":"refused","reason":{"kind":"notPermitted"}}),
            json!({"result":"refused","reason":{"kind":"machineInAnotherOrganization"}}),
            json!({"result":"refused","reason":{"kind":"unknownWorkspace"}}),
            json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":900}}),
        ],
        false,
    );
}

#[test]
fn device_login_refusal_wire_contract() {
    contract_artifacts::assert_contract::<DeviceLoginRefusal>(
        "DeviceLoginRefusal",
        vec![
            json!({"kind":"notPermitted"}),
            json!({"kind":"machineInAnotherOrganization"}),
            json!({"kind":"unknownWorkspace"}),
            json!({"kind":"secondFactorRequired","withinSeconds":900}),
        ],
        false,
    );
}

#[test]
fn decide_machine_token_wire_contract() {
    contract_artifacts::assert_contract::<DecideMachineToken>(
        "DecideMachineToken",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","clientId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn machine_token_decision_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenDecision>(
        "MachineTokenDecision",
        vec![
            json!({"result":"grant","machineId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","decidedAt":1793448000_i64,"grantExpiresAt":1793448600_i64}),
            json!({"result":"grant","machineId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","decidedAt":1793448000_i64,"grantExpiresAt":1793448600_i64,"superseded":{"enrollmentId":"019a3dce-2000-7000-8000-000000000003","clientId":"019a3dce-2000-7000-8000-000000000003"}}),
            json!({"result":"refuse","reason":{"kind":"enrollmentNotActive"}}),
            json!({"result":"refuse","reason":{"kind":"superseded"}}),
            json!({"result":"refuse","reason":{"kind":"wrongClient"}}),
            json!({"result":"refuse","reason":{"kind":"idleExpired"}}),
        ],
        false,
    );
}

#[test]
fn machine_token_refusal_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenRefusal>(
        "MachineTokenRefusal",
        vec![
            json!({"kind":"enrollmentNotActive"}),
            json!({"kind":"superseded"}),
            json!({"kind":"wrongClient"}),
            json!({"kind":"idleExpired"}),
        ],
        false,
    );
}

#[test]
fn presented_machine_wire_contract() {
    contract_artifacts::assert_contract::<PresentedMachine>(
        "PresentedMachine",
        vec![json!({"machineId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn machine_connection_refusal_wire_contract() {
    contract_artifacts::assert_contract::<MachineConnectionRefusal>(
        "MachineConnectionRefusal",
        vec![
            json!({"kind":"unknownMachine"}),
            json!({"kind":"enrollmentNotActive"}),
            json!({"kind":"superseded"}),
            json!({"kind":"machineInAnotherOrganization"}),
            json!({"kind":"wrongClient"}),
        ],
        false,
    );
}

#[test]
fn disable_machine_client_wire_contract() {
    contract_artifacts::assert_contract::<DisableMachineClient>(
        "DisableMachineClient",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","clientId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn disabled_machine_client_wire_contract() {
    contract_artifacts::assert_contract::<DisabledMachineClient>(
        "DisabledMachineClient",
        vec![
            json!({"result":"disabled"}),
            json!({"result":"alreadyDisabled"}),
            json!({"result":"notThisEnrollment"}),
        ],
        false,
    );
}

#[test]
fn machine_principal_wire_contract() {
    contract_artifacts::assert_contract::<MachinePrincipal>(
        "MachinePrincipal",
        vec![
            json!({"kind":"machine","machine_id":"019a3dce-2000-7000-8000-000000000001","enrollment_id":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn machine_token_claims_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenClaims>(
        "MachineTokenClaims",
        vec![
            json!({"principal":{"kind":"machine","machine_id":"019a3dce-2000-7000-8000-000000000001","enrollment_id":"019a3dce-2000-7000-8000-000000000002"},"organization_id":"019a3dce-2000-7000-8000-000000000001","grant_expires_at":1793448600_i64}),
        ],
        true,
    );
}
