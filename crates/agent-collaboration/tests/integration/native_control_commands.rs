use serde_json::{Value, json};

use agent_session_inventory::test_support::fake_api_support;
use fake_api_support::{FakeCollaborationApi, FakeReply};

/// One available Codex endpoint with a native carrier at generation 1.
fn native_inventory(service_id: &str, epoch: &str) -> Value {
    json!({"serviceEpoch":epoch,"sequence":0,"endpoints":[{
        "endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"label":"Fixture Codex",
        "availability":{"state":"available","observedAt":"2026-09-05T12:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
            "schemaDigest":null,"generation":{"serviceEpoch":epoch,"generation":1}}]
    }]})
}

#[tokio::test]
async fn interrupt_cli_reports_unknown_when_the_api_disconnects_after_submission() {
    // Arrange: a stand-in API accepts the exact command and drops its response.
    let service_id = "00000000-0000-4000-8000-000000000001";
    let epoch = "00000000-0000-4000-8000-000000000002";
    let mut fixture = FakeCollaborationApi::new(service_id, epoch)
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let served = fixture.serve(vec![
        FakeReply::Result(native_inventory(service_id, epoch)),
        FakeReply::Disconnect,
    ]);
    // Act.
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "turn",
                "interrupt",
                "--endpoint",
                "codex-local",
                "--session",
                "proof-thread",
                "--turn",
                "proof-turn",
                "--json",
                "--service-directory",
            ])
            .arg(fixture.directory())
            .output(),
    )
    .await
    .unwrap_or_else(|error| panic!("command deadline: {error}"))
    .unwrap_or_else(|error| panic!("command: {error}"));
    let calls = served
        .await
        .unwrap_or_else(|error| panic!("fixture: {error}"))
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    // Assert.
    let tools: Vec<&Value> = calls.iter().map(|call| &call["tool"]).collect();
    assert_eq!(tools, [&json!("endpoints_list"), &json!("turn_interrupt")]);
    let interrupt = &calls[1]["arguments"];
    assert_eq!(interrupt["target"]["sessionId"], "proof-thread");
    assert_eq!(interrupt["turnId"], "proof-turn");
    assert_eq!(interrupt["generation"]["generation"], 1);
    assert_eq!(output.status.code(), Some(5));
    let result: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("output: {error}"));
    assert_eq!(result["error"]["kind"], "outcomeUnknown");
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn session_inspect_cli_preserves_native_rejection_message() {
    let service_id = "00000000-0000-4000-8000-000000000001";
    let epoch = "00000000-0000-4000-8000-000000000002";
    let mut fixture = FakeCollaborationApi::new(service_id, epoch)
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let message = "native thread is unreadable: fixture refusal";
    let peer = fixture.serve(vec![
        FakeReply::Result(native_inventory(service_id, epoch)),
        FakeReply::Error(json!({
            "code":-32050,"message":message,"data":{
                "kind":"nativeRejected","stage":"inspect","message":message,
                "reason":"unknown","nextAction":"inspectTarget","nativeCode":-32600
            }
        })),
    ]);

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "session",
                "inspect",
                "--endpoint",
                "codex-local",
                "--session",
                "unreadable-thread",
                "--json",
                "--service-directory",
            ])
            .arg(fixture.directory())
            .output(),
    )
    .await
    .unwrap_or_else(|error| panic!("command deadline: {error}"))
    .unwrap_or_else(|error| panic!("command: {error}"));
    let calls = peer
        .await
        .unwrap_or_else(|error| panic!("API fixture: {error}"))
        .unwrap_or_else(|error| panic!("API fixture: {error}"));
    let tools: Vec<&Value> = calls.iter().map(|call| &call["tool"]).collect();
    assert_eq!(tools, [&json!("endpoints_list"), &json!("session_inspect")]);

    assert_eq!(
        output.status.code(),
        Some(4),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    assert_eq!(
        result["error"]["message"],
        "native thread is unreadable: fixture refusal"
    );
    assert!(output.stderr.is_empty());
}
