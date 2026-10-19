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
