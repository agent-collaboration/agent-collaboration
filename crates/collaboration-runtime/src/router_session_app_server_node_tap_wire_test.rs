//! N1 tap recording at the CLI edge: every frame on one Codex CLI connection, exactly as it
//! crossed, in both directions from the first one; or, for the dispatcher cases, the request
//! value handed to the dispatcher and the values it returned.
use std::collections::HashMap;

use codex_router_test_support::node_taps::{EdgeParty, MessageFamily, RecordLayer, TapCase};
use serde_json::Value;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

/// The clock reads N1 stamps on a frame: replaced in the compared copy, kept in the raw frame.
pub(crate) const CLOCK_FIELDS: &[&str] = &["startedAtMs", "completedAtMs"];

/// The kind of N1's answer to the CLI's `method` request.
pub(crate) fn response_kind(method: &str, response: &Value) -> String {
    match response.pointer("/error/code") {
        Some(code) => format!("error:{code}:{method}"),
        None => format!("response:{method}"),
    }
}

/// What the recorder is handed: frames read from a real connection, or the values a dispatcher
/// case passes to and gets back from `handle_app_server_rpc`.
#[derive(Clone, Copy, Default)]
enum CliRecordSource {
    /// Frames that crossed the connection loop's socket.
    #[default]
    ConnectionWire,
    /// The request value given to the dispatcher (`TypedInput`) and its returned response and
    /// post-response frames (`TypedOutcome`): never frames on a connection.
    DispatcherValues,
}

impl CliRecordSource {
    fn cli_layer(self) -> RecordLayer {
        match self {
            Self::ConnectionWire => RecordLayer::WireFrame,
            Self::DispatcherValues => RecordLayer::TypedInput,
        }
    }

    fn n1_layer(self) -> RecordLayer {
        match self {
            Self::ConnectionWire => RecordLayer::WireFrame,
            Self::DispatcherValues => RecordLayer::TypedOutcome,
        }
    }
}

/// Records the frames of one CLI connection, pairing each reply with the request it answers.
#[derive(Default)]
pub(crate) struct CliConnectionRecorder {
    source: CliRecordSource,
    /// The CLI's requests still unanswered, by id text.
    cli_requests: HashMap<String, String>,
    /// N1's server requests still unanswered, by id text.
    server_requests: HashMap<String, String>,
}

impl CliConnectionRecorder {
    /// A recorder for a dispatcher case: what it records never crossed a connection.
    pub(crate) fn dispatcher_values() -> Self {
        Self {
            source: CliRecordSource::DispatcherValues,
            ..Self::default()
        }
    }

    /// Records one frame the CLI sent to N1: a request, a notification or a reply.
    pub(crate) fn cli_sent(&mut self, case: &mut TapCase, frame: &Value) {
        let id = frame.get("id").map(Value::to_string);
        match (frame.get("method").and_then(Value::as_str), id) {
            (Some(method), id) => {
                if let Some(id) = id {
                    self.cli_requests.insert(id, method.to_owned());
                }
                case.consumed_raw(
                    self.source.cli_layer(),
                    MessageFamily::CodexClientRequestFromCli,
                    EdgeParty::CodexCli,
                    method,
                    frame.clone(),
                    &[],
                );
            }
            (None, id) => {
                let method = id
                    .and_then(|id| self.server_requests.remove(&id))
                    .unwrap_or_default();
                case.consumed_reply_raw(
                    self.source.cli_layer(),
                    MessageFamily::CodexMessageToCli,
                    EdgeParty::CodexCli,
                    &response_kind(&method, frame),
                    frame.clone(),
                    &[],
                );
            }
        }
    }

    /// Records one frame N1 sent to the CLI: a server request, a notification or a reply.
    pub(crate) fn n1_sent(&mut self, case: &mut TapCase, frame: &Value) {
        let id = frame.get("id").map(Value::to_string);
        match (frame.get("method").and_then(Value::as_str), id) {
            (Some(method), id) => {
                if let Some(id) = id {
                    self.server_requests.insert(id, method.to_owned());
                }
                case.produced_raw(
                    self.source.n1_layer(),
                    MessageFamily::CodexMessageToCli,
                    EdgeParty::CodexCli,
                    method,
                    frame.clone(),
                    CLOCK_FIELDS,
                );
            }
            (None, id) => {
                let method = id
                    .and_then(|id| self.cli_requests.remove(&id))
                    .unwrap_or_default();
                case.produced_reply_raw(
                    self.source.n1_layer(),
                    MessageFamily::CodexClientRequestFromCli,
                    EdgeParty::CodexCli,
                    &response_kind(&method, frame),
                    frame.clone(),
                    CLOCK_FIELDS,
                );
            }
        }
    }

    /// Sends `frame` from the CLI and records it as sent.
    pub(crate) async fn send(
        &mut self,
        case: &mut TapCase,
        client: &mut WebSocketStream<tokio::net::UnixStream>,
        frame: Value,
    ) {
        use futures_util::SinkExt as _;
        client
            .send(Message::Text(frame.to_string().into()))
            .await
            .expect("send frame");
        self.cli_sent(case, &frame);
    }

    /// Reads N1's next text frame, whatever it is, and records it; `None` once the connection
    /// ends.
    pub(crate) async fn next_frame(
        &mut self,
        case: &mut TapCase,
        client: &mut WebSocketStream<tokio::net::UnixStream>,
    ) -> Option<Value> {
        use futures_util::StreamExt as _;
        loop {
            match client.next().await? {
                Ok(Message::Text(text)) => {
                    let frame: Value = serde_json::from_str(&text).expect("json frame");
                    self.n1_sent(case, &frame);
                    return Some(frame);
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }
}
