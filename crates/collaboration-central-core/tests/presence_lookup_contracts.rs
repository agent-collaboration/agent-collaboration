//! Literal wire oracles and serializer-generated artifact freshness.
use collaboration_central_core::identity_contract::{
    AdmittedPersonRow, ConnectionView, EnrollmentView, MachinePresenceAnswer, MachineRow,
    OwnerRefusal, ProjectRef, ReadMachinePresence, SessionRow, SessionStateView, WhoIsLoggedIn,
    WhoIsLoggedInResult, WorkspaceRef,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn read_machine_presence_wire_contract() {
    contract_artifacts::assert_contract::<ReadMachinePresence>(
        "ReadMachinePresence",
        vec![json!({"installation":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn machine_presence_answer_wire_contract() {
    contract_artifacts::assert_contract::<MachinePresenceAnswer>(
        "MachinePresenceAnswer",
        vec![
            json!({"state":"open","connectionId":"019a3dce-2000-7000-8000-000000000001","attachSequence":9007199254740991_i64,"openForMs":7000,"silentForMs":1000}),
            json!({"state":"notConnected"}),
            json!({"state":"notConnected","closedAgoMs":5000}),
        ],
        false,
    );
}

#[test]
fn who_is_logged_in_wire_contract() {
    contract_artifacts::assert_contract::<WhoIsLoggedIn>(
        "WhoIsLoggedIn",
        vec![json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn owner_refusal_wire_contract() {
    contract_artifacts::assert_contract::<OwnerRefusal>(
        "OwnerRefusal",
        vec![
            json!({"kind":"notOwner"}),
            json!({"kind":"notFound"}),
            json!({"kind":"conflict"}),
            json!({"kind":"recordUnwritable"}),
        ],
        false,
    );
}

#[test]
fn enrollment_view_wire_contract() {
    contract_artifacts::assert_contract::<EnrollmentView>(
        "EnrollmentView",
        vec![
            json!({"state":"enrolled","approvedBy":"019a3dce-2000-7000-8000-000000000001","approvedAt":1793448000000_i64}),
            json!({"state":"revoked","approvedBy":"019a3dce-2000-7000-8000-000000000001","approvedAt":1793448000000_i64}),
            json!({"state":"expired","approvedBy":"019a3dce-2000-7000-8000-000000000001","approvedAt":1793448000000_i64}),
        ],
        false,
    );
}

#[test]
fn connection_view_wire_contract() {
    contract_artifacts::assert_contract::<ConnectionView>(
        "ConnectionView",
        vec![
            json!({"state":"connected","since":1793448000000_i64,"lastHeardAt":1793448005000_i64}),
            json!({"state":"notConnected"}),
            json!({"state":"notConnected","lastSeenAt":1793448005000_i64}),
        ],
        false,
    );
}

#[test]
fn workspace_ref_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceRef>(
        "WorkspaceRef",
        vec![
            json!({"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}),
        ],
        false,
    );
}

#[test]
fn project_ref_wire_contract() {
    contract_artifacts::assert_contract::<ProjectRef>(
        "ProjectRef",
        vec![json!({"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"})],
        false,
    );
}

#[test]
fn session_state_view_wire_contract() {
    contract_artifacts::assert_contract::<SessionStateView>(
        "SessionStateView",
        vec![json!("working"), json!("idle"), json!("unknown")],
        false,
    );
}

#[test]
fn session_row_wire_contract() {
    contract_artifacts::assert_contract::<SessionRow>(
        "SessionRow",
        vec![
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"working"}),
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"idle"}),
            json!({"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"unknown"}),
        ],
        false,
    );
}

#[test]
fn machine_row_wire_contract() {
    contract_artifacts::assert_contract::<MachineRow>(
        "MachineRow",
        vec![
            json!({"installation":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine","enrollment":{"state":"enrolled","approvedBy":"019a3dce-2000-7000-8000-000000000001","approvedAt":1793448000000_i64},"mayServe":[{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}],"connection":{"state":"connected","since":1793448000000_i64,"lastHeardAt":1793448005000_i64},"sessions":[{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"working"},{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"idle"},{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"unknown"}]}),
        ],
        false,
    );
}

#[test]
fn admitted_person_row_wire_contract() {
    contract_artifacts::assert_contract::<AdmittedPersonRow>(
        "AdmittedPersonRow",
        vec![json!({"personId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn who_is_logged_in_result_wire_contract() {
    contract_artifacts::assert_contract::<WhoIsLoggedInResult>(
        "WhoIsLoggedInResult",
        vec![
            json!({"result":"snapshot","asOf":1793448005000_i64,"machines":[{"installation":"019a3dce-2000-7000-8000-000000000001","label":"fixture machine","enrollment":{"state":"enrolled","approvedBy":"019a3dce-2000-7000-8000-000000000001","approvedAt":1793448000000_i64},"mayServe":[{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"}],"connection":{"state":"connected","since":1793448000000_i64,"lastHeardAt":1793448005000_i64},"sessions":[{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"working"},{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"idle"},{"session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"},"project":{"projectId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture project"},"workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000001","name":"Fixture workspace"},"isolationKind":"host","startedAt":1793448000000_i64,"state":"unknown"}]}],"people":[]}),
            json!({"result":"snapshot","asOf":1793448005000_i64,"machines":[],"people":[{"personId":"019a3dce-2000-7000-8000-000000000001"}]}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
        ],
        false,
    );
}
