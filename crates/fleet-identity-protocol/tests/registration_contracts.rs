//! Literal wire oracles and serializer-generated artifact freshness.
use fleet_identity_protocol::{
    AgentIsolation, EndFleetSession, IsolationKind, LaunchContext, MachineEnvironment,
    RegisterFleetSession, RegisterFleetSessionResult, SessionEndCause, SessionRefusal, StartedBy,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn register_fleet_session_wire_contract() {
    contract_artifacts::assert_contract::<RegisterFleetSession>(
        "RegisterFleetSession",
        vec![
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"routerOperator"},"isolation":{"kind":"host","instanceId":"019a3dce-2000-7000-8000-000000000001"},"launchContext":{"workingDirectory":"/work/item-two"},"machineEnvironment":"physical"}),
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}},"isolation":{"kind":"processSandbox","instanceId":"019a3dce-2000-7000-8000-000000000001"},"launchContext":{"workingDirectory":"/work/item-two","repository":"https://example.invalid/repository.git","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","model":"configured-model"},"machineEnvironment":"cloudVm"}),
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"remoteSession","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"requestId":"019a3dce-2000-7000-8000-000000000002"},"isolation":{"kind":"container","instanceId":"019a3dce-2000-7000-8000-000000000001"},"launchContext":{"workingDirectory":"/work/item-two"},"machineEnvironment":"physical"}),
        ],
        false,
    );
}

#[test]
fn started_by_wire_contract() {
    contract_artifacts::assert_contract::<StartedBy>(
        "StartedBy",
        vec![
            json!({"kind":"routerOperator"}),
            json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}}),
            json!({"kind":"remoteSession","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"requestId":"019a3dce-2000-7000-8000-000000000001"}),
        ],
        false,
    );
}

#[test]
fn register_fleet_session_result_wire_contract() {
    contract_artifacts::assert_contract::<RegisterFleetSessionResult>(
        "RegisterFleetSessionResult",
        vec![
            json!({"result":"registered","organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"result":"refused","reason":{"kind":"projectUnknown"}}),
            json!({"result":"refused","reason":{"kind":"machineNotAllowed"}}),
            json!({"result":"refused","reason":{"kind":"alreadyEnded"}}),
        ],
        false,
    );
}

#[test]
fn session_refusal_wire_contract() {
    contract_artifacts::assert_contract::<SessionRefusal>(
        "SessionRefusal",
        vec![
            json!({"kind":"projectUnknown"}),
            json!({"kind":"machineNotAllowed"}),
            json!({"kind":"alreadyEnded"}),
        ],
        false,
    );
}

#[test]
fn agent_isolation_wire_contract() {
    contract_artifacts::assert_contract::<AgentIsolation>(
        "AgentIsolation",
        vec![
            json!({"kind":"host","instanceId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"kind":"processSandbox","instanceId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"kind":"container","instanceId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"kind":"vm","instanceId":"019a3dce-2000-7000-8000-000000000001"}),
        ],
        false,
    );
}

#[test]
fn isolation_kind_wire_contract() {
    contract_artifacts::assert_contract::<IsolationKind>(
        "IsolationKind",
        vec![
            json!("host"),
            json!("processSandbox"),
            json!("container"),
            json!("vm"),
        ],
        false,
    );
}

#[test]
fn machine_environment_wire_contract() {
    contract_artifacts::assert_contract::<MachineEnvironment>(
        "MachineEnvironment",
        vec![json!("physical"), json!("cloudVm")],
        false,
    );
}

#[test]
fn launch_context_wire_contract() {
    contract_artifacts::assert_contract::<LaunchContext>(
        "LaunchContext",
        vec![
            json!({"workingDirectory":"/work/item-two"}),
            json!({"workingDirectory":"/work/item-two","repository":"https://example.invalid/repository.git","commit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","model":"configured-model"}),
        ],
        false,
    );
}

#[test]
fn end_fleet_session_wire_contract() {
    contract_artifacts::assert_contract::<EndFleetSession>(
        "EndFleetSession",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"cause":"archived"}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"cause":"deleted"}),
        ],
        false,
    );
}

#[test]
fn session_end_cause_wire_contract() {
    contract_artifacts::assert_contract::<SessionEndCause>(
        "SessionEndCause",
        vec![json!("archived"), json!("deleted")],
        false,
    );
}
