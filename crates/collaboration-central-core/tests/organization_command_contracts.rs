//! Literal wire oracles and serializer-generated artifact freshness.
use collaboration_central_core::identity_contract::{
    AllowMachine, AllowSignIn, AllowanceState, CommandRefusal, CommandStatus, CreateWorkspace,
    IdentityLookup, MachineAllowance, MachineLookup, OrganizationCommandResult, RemoveSession,
    RemoveSignIn, RevokeMachineEnrollment, SetOrganizationOwner, SetTokenLifetime,
    TokenLifetimeSeconds, WithdrawMachineAllowance, WorkspaceLookup,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn identity_lookup_wire_contract() {
    contract_artifacts::assert_contract::<IdentityLookup>(
        "IdentityLookup",
        vec![
            json!({"subject":"person","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"subject":"session","organizationId":"019a3dce-2000-7000-8000-000000000001","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}}),
        ],
        false,
    );
}

#[test]
fn workspace_lookup_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceLookup>(
        "WorkspaceLookup",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn machine_lookup_wire_contract() {
    contract_artifacts::assert_contract::<MachineLookup>(
        "MachineLookup",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","installation":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn command_status_wire_contract() {
    contract_artifacts::assert_contract::<CommandStatus>(
        "CommandStatus",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","commandId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn set_organization_owner_wire_contract() {
    contract_artifacts::assert_contract::<SetOrganizationOwner>(
        "SetOrganizationOwner",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","ownerVerifiedEmail":"owner@example.invalid"}),
        ],
        false,
    );
}

#[test]
fn create_workspace_wire_contract() {
    contract_artifacts::assert_contract::<CreateWorkspace>(
        "CreateWorkspace",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}),
        ],
        false,
    );
}

#[test]
fn allow_sign_in_wire_contract() {
    contract_artifacts::assert_contract::<AllowSignIn>(
        "AllowSignIn",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","verifiedEmail":"member@example.invalid"}),
        ],
        false,
    );
}

#[test]
fn remove_sign_in_wire_contract() {
    contract_artifacts::assert_contract::<RemoveSignIn>(
        "RemoveSignIn",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","verifiedEmail":"member@example.invalid"}),
        ],
        false,
    );
}

#[test]
fn allow_machine_wire_contract() {
    contract_artifacts::assert_contract::<AllowMachine>(
        "AllowMachine",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","installation":"019a3dce-2000-7000-8000-000000000002","workspaceId":"019a3dce-2000-7000-8000-000000000001"}),
        ],
        false,
    );
}

#[test]
fn withdraw_machine_allowance_wire_contract() {
    contract_artifacts::assert_contract::<WithdrawMachineAllowance>(
        "WithdrawMachineAllowance",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","installation":"019a3dce-2000-7000-8000-000000000002","workspaceId":"019a3dce-2000-7000-8000-000000000001"}),
        ],
        false,
    );
}

#[test]
fn revoke_machine_enrollment_wire_contract() {
    contract_artifacts::assert_contract::<RevokeMachineEnrollment>(
        "RevokeMachineEnrollment",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","installation":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn remove_session_wire_contract() {
    contract_artifacts::assert_contract::<RemoveSession>(
        "RemoveSession",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}}),
        ],
        false,
    );
}

#[test]
fn set_token_lifetime_wire_contract() {
    contract_artifacts::assert_contract::<SetTokenLifetime>(
        "SetTokenLifetime",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","seconds":60}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","seconds":3600}),
        ],
        false,
    );
}

#[test]
fn command_refusal_wire_contract() {
    contract_artifacts::assert_contract::<CommandRefusal>(
        "CommandRefusal",
        vec![
            json!({"kind":"notOwner"}),
            json!({"kind":"notFound"}),
            json!({"kind":"conflict"}),
            json!({"kind":"recordUnwritable"}),
            json!({"kind":"secondFactorRequired","withinSeconds":900}),
        ],
        false,
    );
}

#[test]
fn organization_command_result_wire_contract() {
    contract_artifacts::assert_contract::<OrganizationCommandResult>(
        "OrganizationCommandResult",
        vec![
            json!({"result":"applied"}),
            json!({"result":"admitted","commandId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
            json!({"result":"refused","reason":{"kind":"notFound"}}),
            json!({"result":"refused","reason":{"kind":"conflict"}}),
            json!({"result":"refused","reason":{"kind":"recordUnwritable"}}),
            json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":900}}),
        ],
        false,
    );
}

#[test]
fn machine_allowance_wire_contract() {
    contract_artifacts::assert_contract::<MachineAllowance>(
        "MachineAllowance",
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002","state":"active"}),
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002","state":"withdrawn"}),
        ],
        false,
    );
}

#[test]
fn allowance_state_wire_contract() {
    contract_artifacts::assert_contract::<AllowanceState>(
        "AllowanceState",
        vec![json!("active"), json!("withdrawn")],
        false,
    );
}

#[test]
fn token_lifetime_seconds_wire_contract() {
    contract_artifacts::assert_contract::<TokenLifetimeSeconds>(
        "TokenLifetimeSeconds",
        vec![json!(60_i64), json!(600_i64), json!(3600_i64)],
        false,
    );
}
