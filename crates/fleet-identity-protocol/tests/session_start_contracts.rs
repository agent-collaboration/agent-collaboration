//! PD aed9bdf5 is the literal oracle; the real authority handler belongs to F2a.
use fleet_identity_protocol::{CheckSessionStart, SessionStartCheck, SessionStartRefusal};
use serde_json::json;

#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn check_session_start_wire_contract() {
    contract_artifacts::assert_contract::<CheckSessionStart>(
        "CheckSessionStart",
        vec![
            json!({"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"routerOperator"}}),
            json!({"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"session","session":{"endpoint":{"serviceId":"019a3dce-2000-7000-8000-000000000002","endpointId":"codex-local"},"sessionId":"fixture-conversation"}}}),
            json!({"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"remoteSession","session":{"endpoint":{"serviceId":"019a3dce-2000-7000-8000-000000000002","endpointId":"codex-local"},"sessionId":"fixture-conversation"},"requestId":"019a3dce-2000-7000-8000-000000000003"}}),
        ],
        false,
    );
}

#[test]
fn session_start_check_wire_contract() {
    contract_artifacts::assert_contract::<SessionStartCheck>(
        "SessionStartCheck",
        vec![
            json!({"result":"allowed","workspaceId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"result":"refused","reason":{"kind":"projectUnknown"}}),
            json!({"result":"refused","reason":{"kind":"machineNotAllowed"}}),
            json!({"result":"refused","reason":{"kind":"differentWorkspace"}}),
            json!({"result":"refused","reason":{"kind":"starterNotActive"}}),
        ],
        false,
    );
}

#[test]
fn session_start_refusal_wire_contract() {
    contract_artifacts::assert_contract::<SessionStartRefusal>(
        "SessionStartRefusal",
        vec![
            json!({"kind":"projectUnknown"}),
            json!({"kind":"machineNotAllowed"}),
            json!({"kind":"differentWorkspace"}),
            json!({"kind":"starterNotActive"}),
        ],
        false,
    );
}

#[test]
fn session_start_check_cannot_infer_a_project_or_preallocate_a_provider_identity() {
    for invalid in [
        json!({"startedBy":{"kind":"routerOperator"},"workingDirectory":"/fixture"}),
        json!({"startedBy":{"kind":"routerOperator"}}),
        json!({"projectId":"019a3dce-2000-7000-8000-000000000001"}),
        json!({"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"routerOperator"},"sessionId":"invented-session"}),
        json!({"projectId":"019a3dce-2000-7000-8000-000000000001","startedBy":{"kind":"routerOperator"},"startedAt":0}),
    ] {
        assert!(serde_json::from_value::<CheckSessionStart>(invalid).is_err());
    }
    assert!(serde_json::from_value::<SessionStartCheck>(json!({"result":"allowed"})).is_err());
    assert!(
        serde_json::from_value::<SessionStartCheck>(
            json!({"result":"refused","reason":{"kind":"unavailable"}})
        )
        .is_err(),
        "unavailable is a transport failure, not an authority decision"
    );
}
