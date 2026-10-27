//! Native source-process proof substitutes the vendor executable, never its
//! receipt reader, source guard or child creation. No Router recovery ID exists.
use super::*;
use serde_json::json;
use std::ffi::OsString;

fn source_request(fixture: &ClaudeProcessFixture) -> serde_json::Value {
    json!({"serviceDirectory":fixture.root.path().join("router/agent-communication"),
        "expectedService":"00000000-0000-4000-8000-000000000001",
        "operation":{"kind":"nativeClaude","serviceEpoch":"00000000-0000-4000-8000-000000000002",
            "action":{"kind":"new","directory":fixture.working_directory,
                "arguments":Vec::<std::ffi::OsString>::new()}}})
}

#[tokio::test]
async fn source_native_missing_receipt_is_unknown_without_router_id_or_second_launch() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let request = source_request(&fixture);
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        fixture
            .command()
            .env("CLAUDE_FIXTURE_NO_RECEIPT", "1")
            .args(["--source-request-v1", &request.to_string()])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("typed native source response");
    assert_eq!(response["kind"], "ready", "{response}");
    let result = &response["reply"]["result"];
    assert_eq!(response["reply"]["kind"], "nativeClaude");
    assert_eq!(
        result["observation"]["kind"], "unknown",
        "missing native output must not become success or absence: {response}"
    );
    assert_eq!(result["source"]["serviceId"], request["expectedService"]);
    assert_eq!(
        result["source"]["serviceEpoch"],
        request["operation"]["serviceEpoch"]
    );
    assert_eq!(result["action"]["kind"], "new");
    assert_eq!(
        result["action"]["directory"],
        json!(fixture.working_directory)
    );
    assert!(
        result.get("operationId").is_none(),
        "native execution creates no Router operation ID"
    );
    assert!(!response.to_string().contains("operationShow"));
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("invocations.txt"))
            .unwrap()
            .lines()
            .count(),
        1,
        "lost native output must never cause a second child launch"
    );
    assert_eq!(
        fixture.health_requests.load(Ordering::SeqCst),
        1,
        "no automatic preflight/recovery retry"
    );
    assert_eq!(fixture.invocation().unwrap().arguments, ["--bg"]);
}

#[tokio::test]
async fn source_native_wrong_service_or_epoch_rejects_before_health_or_native_child() {
    for (field, value, reason) in [
        (
            "/expectedService",
            "00000000-0000-4000-8000-000000000009",
            "wrongService",
        ),
        (
            "/operation/serviceEpoch",
            "00000000-0000-4000-8000-000000000009",
            "staleBinding",
        ),
    ] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let mut request = source_request(&fixture);
        *request.pointer_mut(field).unwrap() = json!(value);
        let output = fixture
            .command()
            .args(["--source-request-v1", &request.to_string()])
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["reason"], reason, "{response}");
        assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 0);
        assert!(!fixture.root.path().join("invocations.txt").exists());
    }
}

#[tokio::test]
async fn source_native_observed_receipt_and_arguments_are_preserved_without_id_conversion() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let mut request = source_request(&fixture);
    let arguments = [
        "--append-system-prompt",
        "-p literal policy",
        "--name",
        "--background",
    ]
    .map(OsString::from)
    .to_vec();
    request["operation"]["action"]["arguments"] = json!(arguments);
    let output = fixture
        .command()
        .args(["--source-request-v1", &request.to_string()])
        .output()
        .await
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &response["reply"]["result"];
    assert_eq!(result["observation"]["kind"], "observed", "{response}");
    assert_eq!(
        result["observation"]["status"],
        json!({"kind":"exited","code":0})
    );
    let receipt: Vec<u8> = serde_json::from_value(result["observation"]["stdout"].clone()).unwrap();
    assert_eq!(receipt, b"bg-fixture-receipt-001\n");
    assert!(
        result.get("operationId").is_none() && result.get("target").is_none(),
        "an opaque vendor receipt must not become a Router ID or conversation SessionRef"
    );
    assert_eq!(
        fixture.invocation().unwrap().arguments,
        [
            "--bg",
            "--append-system-prompt",
            "-p literal policy",
            "--name",
            "--background"
        ]
    );
    assert_eq!(
        fixture.invocation().unwrap().working_directory,
        fixture.working_directory
    );
    assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn terminal_native_job_restore_revalidates_real_api_uuid_and_directory_before_one_child() {
    use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, NativeClaudeJobReader};
    use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
    use collaboration_service::{CollaborationApplication, ServiceIdentity};
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    for (action, state, captured_cwd, session, allowed) in [
        ("resume", "stopped", true, SESSION, true),
        ("fork", "done", true, SESSION, true),
        ("resume", "stopped", false, SESSION, false),
        ("resume", "future-state", true, SESSION, false),
        (
            "resume",
            "stopped",
            true,
            "distinct-background-handle",
            false,
        ),
    ] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let root = fixture.root.path();
        let peers = root.join("peers");
        fs::create_dir(&peers).unwrap();
        let output = root.join("native-jobs.json");
        fs::write(&output,serde_json::to_vec(&json!([{"kind":"background","id":"distinct-background-handle","sessionId":SESSION,"cwd":fixture.working_directory,"state":state,"startedAt":1790162100000_i64}])).unwrap()).unwrap();
        let reader = root.join("native-read");
        codex_router_test_support::node_fixture::write_node_fixture(
            &reader,
            "native-cli-catalogue-agent.ts",
            &serde_json::json!({"kind":"file","output":{"kind":"path","path":output}}),
        )
        .unwrap();
        fs::set_permissions(&reader, fs::Permissions::from_mode(0o700)).unwrap();
        let description=serde_json::from_value(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"label":"Native Claude","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"},"channels":[]})).unwrap();
        let identity = ServiceIdentity::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )
        .unwrap()
        .with_endpoints(vec![description])
        .unwrap()
        .with_claude_code_sessions(Arc::new(ClaudeCodeSessionRegistry::new(peers)))
        .with_claude_background_jobs(Arc::new(NativeClaudeJobReader::for_executable(reader)));
        let service_directory = root.join("router/agent-communication");
        fs::set_permissions(&service_directory, fs::Permissions::from_mode(0o700)).unwrap();
        let api = ServedCollaborationApi::start_with(
            &service_directory,
            CollaborationApplication::new(identity),
            TestServeOptions {
                router_proxy_endpoint: Some(fixture.endpoint),
                ..TestServeOptions::default()
            },
        )
        .await
        .unwrap();
        let mut request = source_request(&fixture);
        let directory = if captured_cwd {
            fixture.working_directory.clone()
        } else {
            root.join("home")
        };
        request["operation"]["action"] = json!({"kind":action,"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":session},"directory":directory,"arguments":Vec::<OsString>::new()});
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            fixture
                .command()
                .args(["--source-request-v1", &request.to_string()])
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if allowed {
            assert!(output.status.success(), "{action}/{state}: {response}");
            let mut expected = vec!["--bg", "--resume", SESSION];
            if action == "fork" {
                expected.push("--fork-session");
            }
            assert_eq!(fixture.invocation().unwrap().arguments, expected);
            assert_eq!(fixture.invocation().unwrap().working_directory, directory);
            assert_eq!(
                response["reply"]["result"]["action"]["target"]["sessionId"],
                SESSION
            );
            assert_eq!(
                fs::read_to_string(root.join("invocations.txt"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        } else {
            assert!(
                !output.status.success(),
                "forbidden restore {state}/{captured_cwd}/{session}: {response}"
            );
            assert!(
                !root.join("invocations.txt").exists(),
                "no child before qualified UUID/directory/lifecycle"
            );
            assert_eq!(fixture.health_requests.load(Ordering::SeqCst), 0);
        }
        api.stop().await.unwrap();
    }
}
