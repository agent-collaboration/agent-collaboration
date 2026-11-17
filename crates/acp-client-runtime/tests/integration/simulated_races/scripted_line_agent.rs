//! An in-process ACP agent for scenario tests (design note §11.8): it
//! answers JSON-RPC lines on its end of a duplex pair and records every
//! method it reads, in order. It offers session/close and session/load (an
//! empty load, no replay), gives each session/new its own ID and an effort
//! setting, answers each prompt at once with end_turn, can hold its answers
//! to setting changes until the test releases them, and refuses anything
//! else as method not found.

use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, WriteHalf};

/// The methods the agent has read, and a signal for each new one.
#[derive(Clone, Default)]
pub(super) struct ScriptedLineAgent {
    methods: Arc<Mutex<Vec<String>>>,
    changed: Arc<tokio::sync::Notify>,
    sessions: Arc<AtomicUsize>,
    hold_settings: Arc<AtomicBool>,
    settings_released: Arc<tokio::sync::Notify>,
}

type AgentWriter = Arc<tokio::sync::Mutex<WriteHalf<tokio::io::DuplexStream>>>;

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

    /// Answers to setting changes wait until `release_setting_answers`.
    pub(super) fn hold_setting_answers(&self) {
        self.hold_settings.store(true, Ordering::SeqCst);
    }

    /// Sends every held setting answer, and answers later ones at once.
    pub(super) fn release_setting_answers(&self) {
        self.hold_settings.store(false, Ordering::SeqCst);
        self.settings_released.notify_waiters();
    }

    async fn serve(&self, connection: tokio::io::DuplexStream) {
        let (reader, writer) = tokio::io::split(connection);
        let writer: AgentWriter = Arc::new(tokio::sync::Mutex::new(writer));
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
                "session/new" => {
                    let number = self.sessions.fetch_add(1, Ordering::SeqCst) + 1;
                    let session_id = if number == 1 {
                        "scripted-session".to_owned()
                    } else {
                        format!("scripted-session-{number}")
                    };
                    json!({"jsonrpc": "2.0", "id": id, "result": {
                        "sessionId": session_id,
                        "configOptions": effort_options("medium"),
                    }})
                }
                "session/set_config_option" => {
                    let value = frame
                        .pointer("/params/value")
                        .and_then(Value::as_str)
                        .unwrap_or("medium")
                        .to_owned();
                    let reply = json!({"jsonrpc": "2.0", "id": id, "result": {
                        "configOptions": effort_options(&value),
                    }});
                    if self.hold_settings.load(Ordering::SeqCst) {
                        let held = self.clone();
                        let writer = Arc::clone(&writer);
                        tokio::spawn(async move {
                            held.settings_released_now().await;
                            let _sent = write_line(&writer, &reply).await;
                        });
                        continue;
                    }
                    reply
                }
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
            if !write_line(&writer, &reply).await {
                return;
            }
        }
    }

    /// Resolves once setting answers are no longer held.
    async fn settings_released_now(&self) {
        loop {
            let released = self.settings_released.notified();
            if !self.hold_settings.load(Ordering::SeqCst) {
                return;
            }
            released.await;
        }
    }
}

/// The agent's one effort setting, currently `value`.
fn effort_options(value: &str) -> Value {
    json!([{
        "id": "effort", "name": "Effort", "category": "thought_level", "type": "select",
        "currentValue": value,
        "options": [{"value": "medium", "name": "Medium"}, {"value": "high", "name": "High"}],
    }])
}

/// Writes one JSON-RPC line; false once the client end has closed.
async fn write_line(writer: &AgentWriter, reply: &Value) -> bool {
    let mut encoded = reply.to_string();
    encoded.push('\n');
    let mut writer = writer.lock().await;
    writer.write_all(encoded.as_bytes()).await.is_ok() && writer.flush().await.is_ok()
}
