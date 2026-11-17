use super::*;
use serde_json::json;

fn payload(value: serde_json::Value) -> RequestPayload {
    serde_json::from_value(value).unwrap()
}

const ID: &str = "019f0000-0000-7000-8000-0000000a0001";
const THREAD: &str = "019f0000-0000-7000-8000-0000000a0002";
const WORKSPACE: &str = "019f0000-0000-7000-8000-0000000a0003";

#[test]
fn a_thread_post_and_a_direct_message_are_outbox_writes_and_commands_are_not() {
    let post = payload(json!({"kind": "workspace", "request": {
        "requestId": ID, "workspaceId": WORKSPACE,
        "operation": {"kind": "threadPost", "threadId": THREAD, "body": "hi", "addressees": []}}}));
    let message = payload(
        json!({"kind": "mail", "request": {"requestId": ID, "operation": {
        "kind": "sendMessage",
        "to": {"machineId": WORKSPACE, "endpoint": "codex-local", "sessionId": "s1"},
        "body": "hello"}}}),
    );
    let create_thread = payload(json!({"kind": "workspace", "request": {
        "requestId": ID, "workspaceId": WORKSPACE,
        "operation": {"kind": "createThread", "areaId": THREAD, "title": "t", "body": "opening", "addressees": []}}}));

    let post = OutboxWriteContent::of(&post).unwrap();
    let message = OutboxWriteContent::of(&message).unwrap();

    assert!(matches!(
        post.target,
        OutboxWriteTarget::Thread { ref thread_id, .. } if thread_id.as_str() == THREAD
    ));
    assert_eq!(post.body.as_str(), "hi");
    assert!(matches!(message.target, OutboxWriteTarget::Session(_)));
    assert_eq!(message.body.as_str(), "hello");
    assert_eq!(OutboxWriteContent::of(&create_thread), None);
}
