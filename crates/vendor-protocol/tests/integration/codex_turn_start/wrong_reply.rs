use vendor_protocol::codex::{CodexRequest, ThreadReadResponse, TurnStartParams};

fn main() {
    let _: serde_json::Result<ThreadReadResponse> = TurnStartParams::decode_response(
        br#"{"turn":{"id":"turn-fixture","items":[],"status":"inProgress"}}"#,
    );
}
