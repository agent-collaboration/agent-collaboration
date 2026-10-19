//! Actual API target qualification, without executing a vendor or routed preflight.
use super::*;
use serde_json::{Value, json};
#[path = "../../tests/fake_api_support/mod.rs"]
mod fake_api_support;
use fake_api_support::{FakeCollaborationApi, FakeReply};
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";
fn membership(endpoint_id: &str) -> Value {
    json!({"endpoint":{"serviceId":SERVICE,"endpointId":endpoint_id},"label":"Unavailable managed ACP","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"},"channels":[]})
}
fn request(directory: &Path) -> SourceRequest {
    SourceRequest {
        expected_service: SERVICE.to_owned().try_into().unwrap(),
        service_directory: directory.display().to_string().try_into().unwrap(),
        operation: SourceOperation::NativeClaude {
            service_epoch: EPOCH.to_owned().try_into().unwrap(),
            action: SourceNativeClaudeAction::New {
                directory: "/unused".to_owned().try_into().unwrap(),
                arguments: Vec::new(),
            },
        },
    }
}
#[tokio::test]
async fn native_target_qualification_reads_only_canonical_loaded_interactive_origin() {
    let mut api = FakeCollaborationApi::new(SERVICE, EPOCH).unwrap();
    let request = request(api.directory());
    let target:SessionRef=serde_json::from_value(json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"owned-native"})).unwrap();
    let serving=api.serve_then_watch(vec![
        FakeReply::Result(json!({"serviceEpoch":EPOCH,"sequence":1,"endpoints":[membership("claude-local")]})),
        FakeReply::Result(json!({"endpoint":target.endpoint,"observedAt":"2026-10-18T00:00:00Z","nextCursor":null,"sessions":[{
            "origin":"claudeCodeInteractive","target":target,"workingDirectory":"/native","name":"Owned native","status":"idle","startedAt":1,"updatedAt":2,"statusUpdatedAt":2,"kind":"interactive","entrypoint":"cli"}]})),
    ],Duration::from_millis(25));
    let peer = tokio::spawn(serving);
    validate_native_target(&request, &EPOCH.to_owned().try_into().unwrap(), &target)
        .await
        .unwrap();
    let transcript = peer.await.unwrap().unwrap().unwrap();
    assert_eq!(transcript.calls.len(), 2);
    assert!(transcript.further_call.is_none());
    let read = transcript.calls.get(1).unwrap();
    assert_eq!(
        read.pointer("/arguments/view").and_then(Value::as_str),
        Some("loaded")
    );
    assert_eq!(
        read.pointer("/arguments/source").and_then(Value::as_str),
        Some("interactive"),
        "native validation must not access managed store/mixed pagination"
    );
}
#[tokio::test]
async fn native_target_rejects_noncanonical_or_duplicate_membership_before_session_read() {
    for (endpoint_id, descriptions) in [
        ("label-claude", vec![membership("label-claude")]),
        (
            "claude-local",
            vec![membership("claude-local"), membership("claude-local")],
        ),
    ] {
        let mut api = FakeCollaborationApi::new(SERVICE, EPOCH).unwrap();
        let request = request(api.directory());
        let target:SessionRef=serde_json::from_value(json!({"endpoint":{"serviceId":SERVICE,"endpointId":endpoint_id},"sessionId":"owned-native"})).unwrap();
        let serving = api.serve_then_watch(
            vec![FakeReply::Result(
                json!({"serviceEpoch":EPOCH,"sequence":1,"endpoints":descriptions}),
            )],
            Duration::from_millis(25),
        );
        let peer = tokio::spawn(serving);
        assert!(
            validate_native_target(&request, &EPOCH.to_owned().try_into().unwrap(), &target)
                .await
                .is_err()
        );
        let transcript = peer.await.unwrap().unwrap().unwrap();
        assert_eq!(transcript.calls.len(), 1);
        assert!(
            transcript.further_call.is_none(),
            "invalid canonical membership must not issue native/managed session reads"
        );
    }
}
