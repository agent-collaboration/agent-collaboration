use collaboration_contracts::MachineIdentity;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::UuidIdentity;
use collaboration_service::ApplicationFixture;

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;

const SERVICE_ID: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";
const SERVICE_EPOCH: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
mod fake_api_support;

#[tokio::test]
async fn whoami_displays_service_id_and_machine_label_in_text_and_json() {
    let root = tempfile::tempdir().expect("temporary service directory");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private service directory");
    // The CLI reads the machine label from the version 3 manifest the API publishes.
    let machine_identity = MachineIdentity::new(
        UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service id"),
        Some("Sunbook-Pro-M4"),
    )
    .expect("machine label");
    let peer = collaboration_runtime::load_installation_peer(root.path())
        .expect("persisted installation peer");
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_EPOCH)
        .expect("service identity")
        .with_installation_peer(peer)
        .expect("installation identity")
        .with_machine_identity(machine_identity)
        .expect("machine identity");
    let served = ServedCollaborationApi::start(root.path(), (identity).build())
        .expect("served collaboration API");

    let text_output = run_whoami(root.path(), false)
        .await
        .expect("text whoami process");
    assert!(text_output.status.success());
    let text = String::from_utf8(text_output.stdout).expect("UTF-8 text whoami output");
    assert!(text.contains(&format!("machine: {SERVICE_ID} · Sunbook-Pro-M4")));
    assert!(text.contains(&format!("peer: {peer}")));

    let json_output = run_whoami(root.path(), true)
        .await
        .expect("JSON whoami process");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(&json_output.stdout).expect("JSON whoami output");
    assert_eq!(json["result"]["record"]["machineId"], SERVICE_ID);
    assert_eq!(json["result"]["record"]["machineLabel"], "Sunbook-Pro-M4");
    assert_eq!(json["result"]["record"]["peerId"], peer.to_string());

    served.stop().await.expect("collaboration API stops");
}

#[tokio::test]
async fn whoami_refuses_an_installation_rpc_from_a_different_service_identity() {
    let root = tempfile::tempdir().expect("mismatched service fixture");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private service fixture");
    let peer = collaboration_runtime::load_installation_peer(root.path()).expect("owned peer");
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_EPOCH)
        .expect("identity")
        .with_installation_peer(peer)
        .expect("peer identity");
    let served = ServedCollaborationApi::start(root.path(), (identity).build())
        .expect("served actual collaboration API");
    let path = root.path().join("service.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("manifest")).expect("manifest JSON");
    manifest["serviceId"] = serde_json::json!("00000000-0000-4000-8000-000000000003");
    std::fs::write(
        &path,
        serde_json::to_vec(&manifest).expect("mismatched manifest"),
    )
    .expect("publish mismatch fixture");
    let output = run_whoami(root.path(), true)
        .await
        .expect("real whoami process");
    assert!(!output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).expect("typed refusal JSON");
    assert!(
        answer
            .to_string()
            .contains("installation identity belongs to another Router"),
        "{answer}"
    );
    assert!(answer.pointer("/result/record/peerId").is_none());
    served.stop().await.expect("API stops");
}

#[tokio::test]
async fn older_router_keeps_endpoint_discovery_and_reports_the_missing_admin_tool() {
    use fake_api_support::{FakeCollaborationApi, FakeReply};
    use serde_json::json;
    let mut router =
        FakeCollaborationApi::new(SERVICE_ID, SERVICE_EPOCH).expect("older API fixture");
    let inventory = json!({"serviceEpoch":SERVICE_EPOCH,"sequence":0,"endpoints":[]});
    let calls = router.serve(vec![
        FakeReply::Result(inventory.clone()),
        FakeReply::Result(inventory),
        FakeReply::Error(json!({"code":-32602,"message":"tool not found","data":null})),
    ]);
    let discovery = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["endpoints", "list", "--json", "--service-directory"])
        .arg(router.directory())
        .output()
        .await
        .expect("real endpoints executable");
    assert!(
        discovery.status.success(),
        "{}",
        String::from_utf8_lossy(&discovery.stdout)
    );
    let output = run_whoami(router.directory(), true)
        .await
        .expect("real whoami executable");
    assert!(!output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).expect("unsupported tool JSON");
    assert_eq!(answer["error"]["kind"], "unsupportedCapability");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown tool installation_show")),
        "{answer}"
    );
    let calls = tokio::time::timeout(std::time::Duration::from_secs(5), calls)
        .await
        .expect("bounded older API calls")
        .expect("older API task")
        .expect("older API requests");
    assert_eq!(
        calls,
        vec![
            json!({"tool":"endpoints_list","arguments":{}}),
            json!({"tool":"endpoints_list","arguments":{}}),
            json!({"tool":"installation_show","arguments":{}}),
        ]
    );
}

async fn run_whoami(
    service_directory: &std::path::Path,
    machine_output: bool,
) -> Result<std::process::Output, std::io::Error> {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"));
    command
        .arg("whoami")
        .arg("--service-directory")
        .arg(service_directory)
        .env("CODEX_THREAD_ID", "whoami-machine-proof")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CURSOR_CONVERSATION_ID");
    if machine_output {
        command.arg("--json");
    }
    command.output().await
}
