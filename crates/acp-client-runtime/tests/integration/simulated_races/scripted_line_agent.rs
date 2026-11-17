//! An in-process ACP agent for scenario tests (design note §11.8): it
//! answers JSON-RPC lines on its end of a duplex pair and records every
//! method it reads, in order. It offers session/close and session/load (an
//! empty load, no replay), answers each prompt
//! at once with end_turn, and refuses anything else as method not found.

use serde_json::{Value, json};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

/// The methods the agent has read, and a signal for each new one.
#[derive(Clone, Default)]
pub(super) struct ScriptedLineAgent {
    methods: Arc<Mutex<Vec<String>>>,
    changed: Arc<tokio::sync::Notify>,
}

impl ScriptedLineAgent {
    /// Serves `connection` until it closes.
    pub(super) fn start(
        connection: tokio::io::DuplexStream,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let agent = Self::default();
        let serving = agent.clone();
        let task = tokio::spawn(async move { serving.serve(connection).await });
        (agent, task)
    }

    /// Every method read so far, in order.
    pub(super) fn methods(&self) -> Vec<String> {
        self.methods
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Resolves once the agent has read `method`.
    pub(super) async fn received(&self, method: &str) {
        loop {
            let changed = self.changed.notified();
            if self.methods().iter().any(|seen| seen == method) {
                return;
            }
            changed.await;
        }
    }

    async fn serve(&self, connection: tokio::io::DuplexStream) {
        let (reader, mut writer) = tokio::io::split(connection);
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            self.methods
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(method.to_owned());
            self.changed.notify_waiters();
            let Some(id) = frame.get("id") else {
                continue;
            };
            let reply = match method {
                "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": {"loadSession": true, "sessionCapabilities": {"close": {}}},
                    "agentInfo": {"name": "scripted-line-agent", "version": "1"},
                }}),
                "session/new" => json!({"jsonrpc": "2.0", "id": id, "result": {
                    "sessionId": "scripted-session",
                }}),
                "session/prompt" => json!({"jsonrpc": "2.0", "id": id, "result": {
                    "stopReason": "end_turn",
                }}),
                "session/close" | "session/load" => {
                    json!({"jsonrpc": "2.0", "id": id, "result": {}})
                }
                _ => json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32601, "message": "method not found",
                }}),
            };
            let mut encoded = reply.to_string();
            encoded.push('\n');
            if writer.write_all(encoded.as_bytes()).await.is_err() || writer.flush().await.is_err()
            {
                return;
            }
        }
    }
}
