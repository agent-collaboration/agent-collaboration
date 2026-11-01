//! Literal wire oracles and serializer-generated artifact freshness.
use collaboration_central_core::identity_contract::{
    AllowedMachineRow, CommandStatusResult, IdentityLookupResult, MachineLookupResult,
    WorkspaceLookupResult,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn identity_lookup_result_wire_contract() {
    contract_artifacts::assert_contract::<IdentityLookupResult>(
        "IdentityLookupResult",
        vec![
            json!({"result":"person","personId":"019a3dce-2000-7000-8000-000000000001","standing":{"result":"admitted","role":"owner"}}),
            json!({"result":"person","personId":"019a3dce-2000-7000-8000-000000000001","standing":{"result":"admitted","role":"member"}}),
            json!({"result":"person","personId":"019a3dce-2000-7000-8000-000000000001","standing":{"result":"awaitingOwner"}}),
            json!({"result":"person","personId":"019a3dce-2000-7000-8000-000000000001","standing":{"result":"notAdmitted"}}),
            json!({"result":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"machine":"019a3dce-2000-7000-8000-000000000001","project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolation":{"kind":"host","instanceId":"019a3dce-2000-7000-8000-000000000001"},"machineEnvironment":"physical","launchContext":{"workingDirectory":"/work/item-two"},"startedBy":{"kind":"routerOperator"},"startedAt":1793448000000_i64}),
            json!({"result":"unknown"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
        ],
        false,
    );
}

#[test]
fn workspace_lookup_result_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceLookupResult>(
        "WorkspaceLookupResult",
        vec![
            json!({"result":"workspace","workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"allowedMachines":[{"installation":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine"}]}),
            json!({"result":"unknown"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
        ],
        false,
    );
}

#[test]
fn machine_lookup_result_wire_contract() {
    contract_artifacts::assert_contract::<MachineLookupResult>(
        "MachineLookupResult",
        vec![
            json!({"result":"machine","installation":"019a3dce-2000-7000-8000-000000000001","organizationId":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine","enrollment":{"state":"enrolled","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002"},"approvedAt":1793448000000_i64,"allowedWorkspaces":[{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}],"sharedBucket":[{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}]}),
            json!({"result":"unknown"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
        ],
        false,
    );
}

#[test]
fn command_status_result_wire_contract() {
    contract_artifacts::assert_contract::<CommandStatusResult>(
        "CommandStatusResult",
        vec![
            json!({"result":"pending"}),
            json!({"result":"applied"}),
            json!({"result":"unknown"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
        ],
        false,
    );
}

#[test]
fn allowed_machine_row_wire_contract() {
    contract_artifacts::assert_contract::<AllowedMachineRow>(
        "AllowedMachineRow",
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine"}),
        ],
        false,
    );
}
