//! The fake Codex app-server behind N8's taps, and the recorder of its socket.
//!
//! The fake keeps a live transcript of every frame on its socket, both directions, in the order
//! it saw them. A case records that transcript as wire frames from its first frame on, so the
//! binding's own setup (`thread/resume`) and a started turn's calls are recorded and checked too;
//! the case's own messages are the part of it the catalog row talks about.
use codex_router_test_support::node_taps::{
    EdgeParty, MessageFamily, ProtocolNode, RecordIntent, RecordLayer, TapCase,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

/// The one socket between N8's Codex connection and the fake app-server.
pub(super) const MANAGED_CODEX_CONNECTION: &str = "managed-codex";

/// One scripted step of the fake Codex app-server.
pub(super) enum Step {
    /// Receive one request, check its method, answer with this result.
    Request(&'static str, Value),
    /// Send one frame (a notification or a server request).
    Send(Value),
    /// Receive one frame and answer nothing (a reply to a server request).
    Receive,
}

/// Who sent one frame on the Codex socket.
#[derive(Clone, Copy)]
enum FrameSender {
    /// N8, through its Codex connection.
    Router,
    /// The fake app-server.
    FakeCodex,
}

/// Every frame on the socket so far, shared with the test while the fake runs.
#[derive(Clone, Default)]
pub(super) struct CodexTranscript(Arc<Mutex<Vec<(FrameSender, Value)>>>);

impl CodexTranscript {
    fn push(&self, sender: FrameSender, frame: Value) {
        self.0
            .lock()
            .unwrap_or_else(|error| panic!("transcript lock: {error}"))
            .push((sender, frame));
    }

    fn frames(&self) -> Vec<(FrameSender, Value)> {
        self.0
            .lock()
            .unwrap_or_else(|error| panic!("transcript lock: {error}"))
            .clone()
    }
}

/// Runs `script` on the server side of an open WebSocket: a raw socket pair in the unit cases,
/// or a socket accepted from the adapter's own backend connection in the integration case.
pub(super) fn spawn_fake_codex(
    mut socket: WebSocketStream<tokio::net::UnixStream>,
    script: Vec<Step>,
    transcript: CodexTranscript,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        for step in script {
            match step {
                Step::Send(frame) => {
                    transcript.push(FrameSender::FakeCodex, frame.clone());
                    socket
                        .send(Message::Text(frame.to_string().into()))
                        .await
                        .unwrap_or_else(|error| panic!("send: {error}"));
                }
                Step::Request(_, _) | Step::Receive => {
                    let frame = socket
                        .next()
                        .await
                        .unwrap_or_else(|| panic!("fake Codex expected a frame"))
                        .unwrap_or_else(|error| panic!("frame: {error}"));
                    let value: Value = serde_json::from_str(
                        frame.to_text().unwrap_or_else(|error| panic!("{error}")),
                    )
                    .unwrap_or_else(|error| panic!("JSON: {error}"));
                    transcript.push(FrameSender::Router, value.clone());
                    if let Step::Request(method, result) = step {
                        assert_eq!(value["method"], method);
                        let reply = json!({"id":value["id"],"result":result});
                        transcript.push(FrameSender::FakeCodex, reply.clone());
                        socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .unwrap_or_else(|error| panic!("respond: {error}"));
                    }
                }
            }
        }
        // Drain: whatever else the adapter sent before its connection closed is an extra record.
        while let Some(Ok(frame)) = socket.next().await {
            if let Ok(text) = frame.to_text()
                && let Ok(value) = serde_json::from_str::<Value>(text)
            {
                transcript.push(FrameSender::Router, value);
            }
        }
    })
}

/// Records the fake's transcript into a case, frame by frame, from where it last stopped.
pub(super) struct CodexWireRecorder {
    recorded: usize,
    /// How many of N8's first requests are session setup before the case's own messages.
    setup_requests: usize,
    router_requests_seen: usize,
    /// Ids of setup requests, so their replies are marked setup too.
    setup_ids: std::collections::BTreeSet<String>,
    /// `(sender's request id)` to its method, so each reply is named by what it answers.
    router_requests: BTreeMap<String, String>,
    codex_requests: BTreeMap<String, String>,
}

impl CodexWireRecorder {
    /// A recorder whose first `setup_requests` requests from N8 (and their replies) are fixture
    /// setup: checked and counted, but not part of the catalog row's view.
    pub(super) fn with_setup_requests(setup_requests: usize) -> Self {
        Self {
            recorded: 0,
            setup_requests,
            router_requests_seen: 0,
            setup_ids: std::collections::BTreeSet::new(),
            router_requests: BTreeMap::new(),
            codex_requests: BTreeMap::new(),
        }
    }

    /// Records every frame the transcript gained since the last call, exactly as it crossed.
    ///
    /// N8's requests are Codex requests to Codex; the fake's notifications and server requests
    /// are Codex messages from Codex; a reply travels under its request's row. The fake's
    /// scripted results are minimal stand-ins, not real Codex responses: apparatus.
    pub(super) fn record_new(&mut self, case: &mut TapCase, transcript: &CodexTranscript) {
        let frames = transcript.frames();
        case.connection(MANAGED_CODEX_CONNECTION);
        for (sender, frame) in &frames[self.recorded..] {
            let id = frame.get("id").map(Value::to_string);
            let method = frame.get("method").and_then(Value::as_str);
            let counterpart = EdgeParty::Node(ProtocolNode::N10CodexClient);
            match (sender, method, id) {
                (FrameSender::Router, Some(method), id) => {
                    let is_setup = self.router_requests_seen < self.setup_requests;
                    self.router_requests_seen += 1;
                    if let Some(id) = id {
                        if is_setup {
                            self.setup_ids.insert(id.clone());
                        }
                        self.router_requests.insert(id, method.to_owned());
                    }
                    case.produced_raw(
                        RecordLayer::WireFrame,
                        MessageFamily::CodexRequestToCodex,
                        counterpart,
                        method,
                        frame.clone(),
                        &[],
                    );
                    if is_setup {
                        case.mark_last_setup();
                    }
                }
                (FrameSender::Router, None, id) => {
                    let answered = answered(&self.codex_requests, id.as_ref());
                    case.produced_reply_raw(
                        RecordLayer::WireFrame,
                        MessageFamily::CodexMessageFromCodex,
                        counterpart,
                        &format!("response:{answered}"),
                        frame.clone(),
                        &[],
                    );
                }
                (FrameSender::FakeCodex, Some(method), id) => {
                    if let Some(id) = id {
                        self.codex_requests.insert(id, method.to_owned());
                    }
                    case.consumed_raw(
                        RecordLayer::WireFrame,
                        MessageFamily::CodexMessageFromCodex,
                        counterpart,
                        method,
                        frame.clone(),
                        &[],
                    );
                }
                (FrameSender::FakeCodex, None, id) => {
                    let is_setup = id.as_ref().is_some_and(|id| self.setup_ids.contains(id));
                    let answered = answered(&self.router_requests, id.as_ref());
                    case.consumed_reply_raw(
                        RecordLayer::WireFrame,
                        MessageFamily::CodexRequestToCodex,
                        counterpart,
                        &format!("response:{answered}"),
                        frame.clone(),
                        &[],
                    );
                    case.mark_last(RecordIntent::SyntheticApparatus);
                    if is_setup {
                        case.mark_last_setup();
                    }
                }
            }
        }
        self.recorded = frames.len();
    }
}

fn answered(requests: &BTreeMap<String, String>, id: Option<&String>) -> String {
    id.and_then(|id| requests.get(id))
        .cloned()
        .unwrap_or_else(|| "unknown".to_owned())
}
