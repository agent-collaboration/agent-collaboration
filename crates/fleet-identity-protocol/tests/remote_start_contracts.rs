//! Literal wire oracles and serializer-generated artifact freshness.
use fleet_identity_protocol::{
    HarnessKind, ListWorkspaceMachines, RemoteLaunch, RemoteLaunchAccess, RemoteLaunchPolicy,
    RemoteStartAdmission, RemoteStartAnswer, RemoteStartOutcome, RemoteStartRefusal,
    RemoteStartResult, RemoteStartStatus, ReportRemoteStart, RequestRemoteStart,
    StartRequestedSession, TargetStartRefusal, WorkspaceMachine,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn request_remote_start_wire_contract() {
    contract_artifacts::assert_contract::<RequestRemoteStart>(
        "RequestRemoteStart",
        vec![
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","requester":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000002","projectId":"019a3dce-2000-7000-8000-000000000001","launch":{"harness":"codex","workingDirectory":"/work/item-two","requestedPolicy":{"access":"workspace-write"}},"timeoutSeconds":60}),
        ],
        false,
    );
}

#[test]
fn remote_launch_wire_contract() {
    contract_artifacts::assert_contract::<RemoteLaunch>(
        "RemoteLaunch",
        vec![
            json!({"harness":"codex","workingDirectory":"/work/item-two","requestedPolicy":{"access":"workspace-write"}}),
            json!({"harness":"claude","workingDirectory":"/work/item-two","requestedPolicy":{"access":"workspace-write"}}),
            json!({"harness":"cursor","workingDirectory":"/work/item-two","requestedPolicy":{"access":"workspace-write"}}),
        ],
        false,
    );
}

#[test]
fn harness_kind_wire_contract() {
    contract_artifacts::assert_contract::<HarnessKind>(
        "HarnessKind",
        vec![json!("codex"), json!("claude"), json!("cursor")],
        false,
    );
}

#[test]
fn remote_launch_policy_wire_contract() {
    contract_artifacts::assert_contract::<RemoteLaunchPolicy>(
        "RemoteLaunchPolicy",
        vec![
            json!({"access":"write-restricted"}),
            json!({"access":"workspace-write"}),
        ],
        false,
    );
}

#[test]
fn remote_launch_access_wire_contract() {
    contract_artifacts::assert_contract::<RemoteLaunchAccess>(
        "RemoteLaunchAccess",
        vec![json!("write-restricted"), json!("workspace-write")],
        false,
    );
}

#[test]
fn remote_start_admission_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartAdmission>(
        "RemoteStartAdmission",
        vec![
            json!({"result":"forwarded","requestId":"019a3dce-2000-7000-8000-000000000001","deadline":1793448060_i64}),
            json!({"result":"refused","reason":{"kind":"machineNotConnected"}}),
        ],
        false,
    );
}

#[test]
fn remote_start_refusal_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartRefusal>(
        "RemoteStartRefusal",
        vec![
            json!({"kind":"notMember"}),
            json!({"kind":"projectNotInWorkspace"}),
            json!({"kind":"machineNotAllowed"}),
            json!({"kind":"machineNotConnected"}),
            json!({"kind":"requestIdReused"}),
            json!({"kind":"unavailable"}),
        ],
        false,
    );
}

#[test]
fn start_requested_session_wire_contract() {
    contract_artifacts::assert_contract::<StartRequestedSession>(
        "StartRequestedSession",
        vec![
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","requester":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"workspaceId":"019a3dce-2000-7000-8000-000000000001","projectId":"019a3dce-2000-7000-8000-000000000001","launch":{"harness":"codex","workingDirectory":"/work/item-two","requestedPolicy":{"access":"workspace-write"}},"deadline":1793448060_i64}),
        ],
        false,
    );
}

#[test]
fn report_remote_start_wire_contract() {
    contract_artifacts::assert_contract::<ReportRemoteStart>(
        "ReportRemoteStart",
        vec![
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","outcome":{"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","outcome":{"result":"refusedByTarget","reason":{"kind":"deadlinePassed"}}}),
        ],
        false,
    );
}

#[test]
fn remote_start_outcome_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartOutcome>(
        "RemoteStartOutcome",
        vec![
            json!({"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}}),
            json!({"result":"refusedByTarget","reason":{"kind":"providerCreateFailed"}}),
        ],
        false,
    );
}

#[test]
fn target_start_refusal_wire_contract() {
    contract_artifacts::assert_contract::<TargetStartRefusal>(
        "TargetStartRefusal",
        vec![
            json!({"kind":"workingDirectoryMissing"}),
            json!({"kind":"startCheckRefused"}),
            json!({"kind":"deadlinePassed"}),
            json!({"kind":"providerCreateFailed"}),
        ],
        false,
    );
}

#[test]
fn remote_start_result_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartResult>(
        "RemoteStartResult",
        vec![
            json!({"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"late":false}),
            json!({"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"late":true}),
            json!({"result":"refused","reason":{"kind":"notMember"}}),
            json!({"result":"refusedByTarget","reason":{"kind":"startCheckRefused"}}),
            json!({"result":"unknown"}),
            json!({"result":"pending"}),
        ],
        false,
    );
}

#[test]
fn remote_start_answer_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartAnswer>(
        "RemoteStartAnswer",
        vec![
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"late":false}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"started","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"late":true}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"refused","reason":{"kind":"notMember"}}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"refusedByTarget","reason":{"kind":"startCheckRefused"}}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"unknown"}}),
            json!({"requestId":"019a3dce-2000-7000-8000-000000000001","result":{"result":"pending"}}),
        ],
        false,
    );
}

#[test]
fn remote_start_status_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartStatus>(
        "RemoteStartStatus",
        vec![json!({"requestId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn list_workspace_machines_wire_contract() {
    contract_artifacts::assert_contract::<ListWorkspaceMachines>(
        "ListWorkspaceMachines",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn workspace_machine_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceMachine>(
        "WorkspaceMachine",
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine"}),
        ],
        false,
    );
}
