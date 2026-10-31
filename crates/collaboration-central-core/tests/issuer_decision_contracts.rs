//! Literal wire oracles and serializer-generated artifact freshness.
use collaboration_central_core::identity_contract::{
    AdmitMachineConnection, AdmittedMachine, DecideDeviceLogin, DecideMachineToken,
    DeviceLoginDecision, DeviceLoginRefusal, DisableMachineClient, DisabledMachineClient,
    MachineConnectionAdmission, MachineConnectionRefusal, MachinePrincipal, MachineTokenClaims,
    MachineTokenDecision, MachineTokenRefusal, OrganizationStanding, PersonAdmissionRequest,
    PresentedMachine, StandingRole,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn decide_device_login_wire_contract() {
    contract_artifacts::assert_contract::<DecideDeviceLogin>(
        "DecideDeviceLogin",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","approver":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002","label":"fixture machine","allowedWorkspaces":["019a3dce-2000-7000-8000-000000000001"]}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","approver":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002","label":"fixture machine","allowedWorkspaces":[],"approverSecondFactorAt":1793448000_i64}),
        ],
        false,
    );
}

#[test]
fn device_login_decision_wire_contract() {
    contract_artifacts::assert_contract::<DeviceLoginDecision>(
        "DeviceLoginDecision",
        vec![
            json!({"result":"approved","enrollmentId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
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
            json!({"kind":"notOwner"}),
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
            json!({"result":"grant","installation":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","decidedAt":1793448000_i64,"grantExpiresAt":1793448600_i64}),
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
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn admit_machine_connection_wire_contract() {
    contract_artifacts::assert_contract::<AdmitMachineConnection>(
        "AdmitMachineConnection",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","clientId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","presented":{"installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002"},"tokenExpiresAt":1793448600_i64}),
        ],
        false,
    );
}

#[test]
fn admitted_machine_wire_contract() {
    contract_artifacts::assert_contract::<AdmittedMachine>(
        "AdmittedMachine",
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002","organizationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","tokenExpiresAt":1793448600_i64}),
        ],
        false,
    );
}

#[test]
fn machine_connection_admission_wire_contract() {
    contract_artifacts::assert_contract::<MachineConnectionAdmission>(
        "MachineConnectionAdmission",
        vec![
            json!({"result":"admitted","machine":{"installation":"019a3dce-2000-7000-8000-000000000001","peer":"019a3dce-2000-7000-8000-000000000002","organizationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","tokenExpiresAt":1793448600_i64}}),
            json!({"result":"refused","reason":{"kind":"unknownMachine"}}),
            json!({"result":"refused","reason":{"kind":"enrollmentNotActive"}}),
            json!({"result":"refused","reason":{"kind":"superseded"}}),
            json!({"result":"refused","reason":{"kind":"machineInAnotherOrganization"}}),
            json!({"result":"refused","reason":{"kind":"wrongClient"}}),
        ],
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
fn person_admission_request_wire_contract() {
    contract_artifacts::assert_contract::<PersonAdmissionRequest>(
        "PersonAdmissionRequest",
        vec![
            json!({"operation":"admitSignIn","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"operation":"admitSignIn","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002","verifiedEmail":"fixture@example.invalid"}),
            json!({"operation":"standing","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn organization_standing_wire_contract() {
    contract_artifacts::assert_contract::<OrganizationStanding>(
        "OrganizationStanding",
        vec![
            json!({"result":"admitted","role":"owner"}),
            json!({"result":"admitted","role":"member"}),
            json!({"result":"awaitingOwner"}),
            json!({"result":"notAdmitted"}),
        ],
        false,
    );
}

#[test]
fn standing_role_wire_contract() {
    contract_artifacts::assert_contract::<StandingRole>(
        "StandingRole",
        vec![json!("member"), json!("owner")],
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
            json!({"kind":"machine","installation_id":"019a3dce-2000-7000-8000-000000000001","enrollment_id":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn machine_token_claims_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenClaims>(
        "MachineTokenClaims",
        vec![
            json!({"principal":{"kind":"machine","installation_id":"019a3dce-2000-7000-8000-000000000001","enrollment_id":"019a3dce-2000-7000-8000-000000000002"},"organization_id":"019a3dce-2000-7000-8000-000000000001","grant_expires_at":1793448600_i64}),
        ],
        true,
    );
}
