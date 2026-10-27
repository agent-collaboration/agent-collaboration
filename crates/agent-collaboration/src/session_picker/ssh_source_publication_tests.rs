//! Explicit compiled-source proof: only the SSH server and external API replies
//! are substituted. Source executable, carrier, decoder, loader and UI stay real.
use super::*;
use crate::sessions::fake_api_support::{FakeCollaborationApi, FakeReply};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

const PROBE: &str = "session_picker::picker_component::tests::picker_machine_control_tests::all_status_tests::ssh_source_publication_tests::source_executable_ssh_publication_probe";
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

fn inventory() -> Value {
    json!({"serviceEpoch":EPOCH,"sequence":1,"endpoints":[
        {"endpoint":{"serviceId":SERVICE,"endpointId":"cursor-local"},"label":"Managed source","availability":{"state":"available","observedAt":"2026-10-18T00:00:00Z"},
        "channels":[{"kind":"externalProvider","transport":"stdioAcp","bindingId":"cursor-binding","bindingGeneration":1,"runtime":{"provider":"cursor","runtimeName":"fixture"},"capabilities":[{"name":"create","status":"unsupported","evidence":"advertised"}]}]},
        {"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"label":"Native source","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"No ACP channel"},"channels":[]}
    ]})
}

#[tokio::test]
#[ignore = "Requires SOURCE_EXECUTABLE declaring the actual current compiled public agent-sessions binary"]
async fn source_executable_ssh_publication_preserves_partial_and_invalid_distinction() {
    let binary = PathBuf::from(
        std::env::var_os("SOURCE_EXECUTABLE").expect("declare compiled public agent-sessions"),
    );
    assert!(binary.is_absolute() && binary.is_file());
    let root = tempfile::Builder::new()
        .prefix("source-publication-")
        .tempdir()
        .unwrap();
    let ssh = root.path().join("ssh");
    codex_router_test_support::node_fixture::write_node_fixture(
        &ssh,
        "source-ssh-agent.ts",
        &json!({"kind":"publication"}),
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (mode, code, data) in [
        (
            "unavailable",
            -32050,
            json!({"kind":"unavailable","stage":"discovery","message":"SSH_SOURCE_PRIVATE_SENTINEL"}),
        ),
        (
            "invalid",
            -32602,
            json!({"kind":"unavailable","stage":"discovery","message":"SSH_SOURCE_PRIVATE_SENTINEL"}),
        ),
        (
            "malformed",
            -32050,
            json!({"kind":"futureFailure","stage":"discovery","message":"SSH_SOURCE_PRIVATE_SENTINEL"}),
        ),
    ] {
        let mut api = FakeCollaborationApi::new(SERVICE, EPOCH).unwrap();
        let managed = json!({"origin":"hostedProvider","target":{"endpoint":{"serviceId":SERVICE,"endpointId":"cursor-local"},"sessionId":"stored-source-target"},"workingDirectory":"/source/saved","updatedAt":2,"state":"unloaded","requestedPolicy":{"access":"write-restricted"},"approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}});
        let mut responses = vec![
            FakeReply::Result(inventory()),
            FakeReply::Result(
                json!({"endpoint":managed["target"]["endpoint"],"observedAt":"2026-10-18T00:00:00Z","sessions":[managed],"nextCursor":null}),
            ),
            FakeReply::Result(inventory()),
            FakeReply::Error(
                json!({"code":code,"message":"SSH_SOURCE_PRIVATE_SENTINEL","data":data}),
            ),
        ];
        if mode == "unavailable" {
            responses.push(FakeReply::Result(inventory()));
            responses.push(FakeReply::Error(
                json!({"code":code,"message":"SSH_SOURCE_PRIVATE_SENTINEL","data":data}),
            ));
            responses.push(FakeReply::Result(inventory()));
        }
        let api_peer = api.serve(responses);
        let transcript = root.path().join(format!("{mode}.jsonl"));
        let child = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", PROBE, "--ignored", "--nocapture"])
                .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                .env("SOURCE_EXECUTABLE", &binary)
                .env("SOURCE_SSH_TRANSCRIPT", &transcript)
                .env("SOURCE_API_DIRECTORY", api.directory())
                .env("SOURCE_EXPECTED_MODE", mode)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        if !child.status.success() {
            api_peer.abort();
        }
        assert!(
            child.status.success(),
            "{mode}: {} {}; actual SSH requests: {}; actual source results: {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr),
            std::fs::read_to_string(&transcript).unwrap_or_default(),
            std::fs::read_to_string(format!("{}.results", transcript.display()))
                .unwrap_or_default()
        );
        let calls = tokio::time::timeout(Duration::from_secs(1), api_peer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let requests = std::fs::read_to_string(transcript)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let expected_count = if mode == "unavailable" { 7 } else { 4 };
        assert_eq!(calls.len(), expected_count);
        assert_eq!(requests.len(), expected_count, "no retry or fallback");
        assert_eq!(requests[1]["operation"]["params"]["view"], "stored");
        assert_eq!(requests[3]["operation"]["params"]["view"], "stored");
        if mode == "unavailable" {
            assert_eq!(requests[5]["operation"]["params"]["view"], "loaded");
            assert_eq!(requests[5]["operation"]["params"]["source"], "interactive");
        }
        assert_eq!(requests[3]["operation"]["params"]["source"], "interactive");
        assert!(!String::from_utf8_lossy(&child.stdout).contains("SSH_SOURCE_PRIVATE_SENTINEL"));
        assert!(!String::from_utf8_lossy(&child.stderr).contains("SSH_SOURCE_PRIVATE_SENTINEL"));
    }
}

#[tokio::test]
#[ignore = "Isolated child of the declared compiled-source proof"]
async fn source_executable_ssh_publication_probe() {
    use crate::sessions::router_connection_registry::RouterConnectionRegistry;
    let binary = std::env::var("SOURCE_EXECUTABLE").unwrap();
    let directory = std::env::var("SOURCE_API_DIRECTORY").unwrap();
    let mode = std::env::var("SOURCE_EXPECTED_MODE").unwrap();
    let profile=RouterConnectionRegistry::parse(&json!({"version":1,"routers":[{
        "name":"Observed source","connection":{"kind":"ssh","serviceId":SERVICE,"destination":"source.example.invalid","executable":binary,"serviceDirectory":directory,"clientProfile":"routerDebug"},"defaultRemoteCwd":"/source/new-only"
    }]}).to_string()).unwrap().routers.remove(0);
    let loader = crate::sessions::session_picker_record_loader(
        crate::CliContext::new(Vec::new()),
        collaboration_client::session_catalog::discover_repository_identity(Path::new(&directory)),
        None,
    );
    let marker = if mode == "unavailable" {
        // The per-partition warning can render while Loaded is still in flight.
        // Observe the final publication, after both native partitions settle.
        "Observed source: 1 sessions; Native Claude inventory incomplete"
    } else {
        "Observed source: Unavailable"
    };
    let frame = capture_all_status(
        PickerSourceContext::ConfiguredHosted(profile),
        loader,
        marker,
        None,
    )
    .await;
    if mode == "unavailable" {
        assert!(frame.contains("Managed source"));
        assert!(frame.contains("Partial view"));
    } else {
        assert!(!frame.contains("Managed source"));
        assert!(!frame.contains("Native Claude inventory incomplete"));
    }
    assert!(!frame.contains("SSH_SOURCE_PRIVATE_SENTINEL"));
}
