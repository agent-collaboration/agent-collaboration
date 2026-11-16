//! The live-recording entry point rejects corrupt recordings whole and checks complete ones.
use serde_json::{Value, json};

use super::check_recording;
use crate::node_taps::{EdgeDefinitions, OutsideEdge, OwnedCodecs};

fn line(seq: u64, direction: &str, message: &Value) -> String {
    json!({"seq": seq, "connection": 0, "direction": direction, "message": message}).to_string()
}

fn mcp_recording() -> Vec<String> {
    vec![
        line(
            0,
            "clientToServer",
            &json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}),
        ),
        line(
            1,
            "serverToClient",
            &json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}}, "serverInfo": {"name": "router", "version": "1"}}}),
        ),
    ]
}

fn check(lines: &[String]) -> Result<Value, String> {
    check_recording(
        EdgeDefinitions::shared(),
        OutsideEdge::Mcp,
        "test.jsonl",
        &lines.join("\n"),
        &OwnedCodecs::default(),
    )
}

#[test]
fn a_complete_recording_is_checked_frame_by_frame() {
    let report = check(&mcp_recording()).expect("a complete recording");
    assert_eq!(report["frames"], 2);
    assert_eq!(report["counts"]["conforms"], 2, "{report:#}");
}

#[test]
fn a_recording_with_a_sequence_gap_is_rejected() {
    let mut lines = mcp_recording();
    lines.push(line(
        3,
        "clientToServer",
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ));
    assert!(check(&lines).is_err_and(|problem| problem.contains("seq 3")));
}

#[test]
fn a_truncated_line_is_rejected() {
    let mut lines = mcp_recording();
    let last = lines.pop().expect("a line");
    lines.push(last.chars().take(last.chars().count() / 2).collect());
    assert!(check(&lines).is_err_and(|problem| problem.contains("does not parse")));
}

#[test]
fn a_recording_without_initialize_is_rejected() {
    let lines = vec![line(
        0,
        "clientToServer",
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
    )];
    assert!(check(&lines).is_err_and(|problem| problem.contains("no initialize")));
}

#[test]
fn a_redacted_line_is_counted_not_checked() {
    let mut lines = mcp_recording();
    lines.push(
        json!({"seq": 2, "connection": 0, "direction": "clientToServer",
        "redacted": {"method": "thread/read", "threadIdHash": "ab12"}})
        .to_string(),
    );
    let report = check(&lines).expect("a complete recording");
    assert_eq!(report["checked"], 2);
    assert_eq!(report["redacted"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_request_left_without_its_reply_is_reported_unanswered() {
    let mut lines = mcp_recording();
    lines.push(line(
        2,
        "clientToServer",
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
    ));
    let report = check(&lines).expect("a complete recording");
    assert_eq!(
        report["unanswered"],
        json!([{"connection": "0", "id": "1", "method": "tools/list"}])
    );
}
