//! Literal wire oracles and serializer-generated artifact freshness.
use fleet_identity_protocol::{
    Activity, ActivityRefusal, Heartbeat, MachineCapability, RenewMachineToken,
    ReportSessionActivity, ReportSessionActivityResult, SessionActivityEntry,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn heartbeat_wire_contract() {
    contract_artifacts::assert_contract::<Heartbeat>(
        "Heartbeat",
        vec![
            json!({"capabilities":[]}),
            json!({"capabilities":[{"kind":"remoteStart"}]}),
        ],
        false,
    );
}

#[test]
fn machine_capability_wire_contract() {
    contract_artifacts::assert_contract::<MachineCapability>(
        "MachineCapability",
        vec![json!({"kind":"remoteStart"})],
        false,
    );
}

#[test]
fn report_session_activity_wire_contract() {
    contract_artifacts::assert_contract::<ReportSessionActivity>(
        "ReportSessionActivity",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","sequence":0,"sessions":[]}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","sequence":9007199254740991_i64,"sessions":[{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"activity":"working"}]}),
        ],
        false,
    );
}

#[test]
fn session_activity_entry_wire_contract() {
    contract_artifacts::assert_contract::<SessionActivityEntry>(
        "SessionActivityEntry",
        vec![
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"activity":"working"}),
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"activity":"idle"}),
        ],
        false,
    );
}

#[test]
fn activity_wire_contract() {
    contract_artifacts::assert_contract::<Activity>(
        "Activity",
        vec![json!("working"), json!("idle")],
        false,
    );
}

#[test]
fn report_session_activity_result_wire_contract() {
    contract_artifacts::assert_contract::<ReportSessionActivityResult>(
        "ReportSessionActivityResult",
        vec![
            json!({"result":"accepted"}),
            json!({"result":"superseded"}),
            json!({"result":"refused","reason":{"kind":"notCurrentConnection"}}),
        ],
        false,
    );
}

#[test]
fn activity_refusal_wire_contract() {
    contract_artifacts::assert_contract::<ActivityRefusal>(
        "ActivityRefusal",
        vec![json!({"kind":"notCurrentConnection"})],
        false,
    );
}

#[test]
fn renew_machine_token_wire_contract() {
    contract_artifacts::assert_contract::<RenewMachineToken>(
        "RenewMachineToken",
        vec![json!({"token":"structural-token-not-a-credential"})],
        false,
    );
}
