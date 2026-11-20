use vendor_protocol::codex::{
    CodexServerNotification, TurnCompletedNotification, TurnStartedNotification,
};

fn main() {
    let payload = br#"{"threadId":"thread-fixture","turn":{"id":"turn-fixture","items":[],"status":"inProgress"}}"#;
    let _: serde_json::Result<TurnCompletedNotification> =
        TurnStartedNotification::decode_params(payload);
    let _: serde_json::Result<TurnStartedNotification> =
        TurnCompletedNotification::decode_params(payload);
}
