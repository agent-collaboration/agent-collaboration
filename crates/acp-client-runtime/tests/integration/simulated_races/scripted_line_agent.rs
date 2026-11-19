//! An in-process ACP agent for scenario tests (design note §11.8): it
//! answers JSON-RPC lines on its end of a duplex pair and records every
//! method it reads, in order. It offers session/close and session/load (an
//! empty load, no replay), gives each session/new its own ID and an effort
//! setting, answers each prompt at once with end_turn, can hold its answers
//! to setting changes until the test releases them, and refuses anything
//! else as method not found. A scenario can also hold its answers to
//! session/load, have a load come back in a given mode (a select option
//! "mode": default, plan, auto), read every mode change it was sent, and
//! send its own session/request_permission for a session.

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
    settings_answered: Arc<AtomicUsize>,
    hold_loads: Arc<AtomicBool>,
    loads_released: Arc<tokio::sync::Notify>,
    restored_mode: Arc<Mutex<Option<String>>>,
    mode_sets: Arc<Mutex<Vec<String>>>,
    prompt_sessions: Arc<Mutex<Vec<String>>>,
    mode_reply: Arc<Mutex<Option<ModeReply>>>,
    writer: Arc<Mutex<Option<AgentWriter>>>,
    responses: Arc<Mutex<Vec<Value>>>,
}

type AgentWriter = Arc<tokio::sync::Mutex<WriteHalf<tokio::io::DuplexStream>>>;

#[derive(Clone)]
pub(super) enum ModeReply {
    Selected(String),
    NoCatalogue,
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

    /// Answers to setting changes wait until `release_setting_answers`.
    pub(super) fn hold_setting_answers(&self) {
        self.hold_settings.store(true, Ordering::SeqCst);
    }

    /// Sends every held setting answer, and answers later ones at once.
    pub(super) fn release_setting_answers(&self) {
        self.hold_settings.store(false, Ordering::SeqCst);
        self.settings_released.notify_waiters();
    }

    pub(super) async fn setting_answers_sent(&self, count: usize) {
        loop {
            let changed = self.changed.notified();
            if self.settings_answered.load(Ordering::SeqCst) >= count {
                return;
            }
            changed.await;
        }
    }

    /// Answers to session/load wait until `release_load_answers`.
    pub(super) fn hold_load_answers(&self) {
        self.hold_loads.store(true, Ordering::SeqCst);
    }

    /// Sends every held load answer, and answers later ones at once.
    pub(super) fn release_load_answers(&self) {
        self.hold_loads.store(false, Ordering::SeqCst);
        self.loads_released.notify_waiters();
    }

    /// Every session/load comes back with the session in `mode`.
    pub(super) fn restore_in_mode(&self, mode: &str) {
        *self
            .restored_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(mode.to_owned());
    }

    pub(super) fn next_mode_reply(&self, reply: ModeReply) {
        *self
            .mode_reply
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(reply);
    }

    /// Every mode value the agent was asked to switch to, in order.
    pub(super) fn mode_sets(&self) -> Vec<String> {
        self.mode_sets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) async fn mode_sets_received(&self, count: usize) {
        loop {
            let changed = self.changed.notified();
            if self.mode_sets().len() >= count {
                return;
            }
            changed.await;
        }
    }

    pub(super) fn prompt_sessions(&self) -> Vec<String> {
        self.prompt_sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends session/request_permission for `session_id` and answers the
    /// client's response: its result, or its error.
    pub(super) async fn request_permission(&self, session_id: &str) -> Value {
        let request_id = "agent-permission";
        let request = json!({"jsonrpc": "2.0", "id": request_id, "method": "session/request_permission", "params": {
            "sessionId": session_id,
            "toolCall": {"toolCallId": "scripted-tool", "title": "Scripted action", "kind": "edit"},
            "options": [
                {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
            ],
        }});
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("the agent is serving");
        assert!(
            write_line(&writer, &request).await,
            "the client end is open"
        );
        loop {
            let changed = self.changed.notified();
            if let Some(response) = self
                .responses
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|response| response.get("id") == Some(&json!(request_id)))
                .cloned()
            {
                return response;
            }
            changed.await;
        }
    }

    async fn serve(&self, connection: tokio::io::DuplexStream) {
        let (reader, writer) = tokio::io::split(connection);
        let writer: AgentWriter = Arc::new(tokio::sync::Mutex::new(writer));
        *self.writer.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&writer));
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                // A response to a request the agent sent.
                self.responses
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(frame);
                self.changed.notify_waiters();
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
                "session/set_config_option"
                    if frame.pointer("/params/configId").and_then(Value::as_str)
                        == Some("mode") =>
                {
                    let value = frame
                        .pointer("/params/value")
                        .and_then(Value::as_str)
                        .unwrap_or("default")
                        .to_owned();
                    self.mode_sets
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(value.clone());
                    self.changed.notify_waiters();
                    let result = match self
                        .mode_reply
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                    {
                        Some(ModeReply::Selected(mode)) => {
                            json!({"configOptions": mode_and_effort_options(&mode)})
                        }
                        Some(ModeReply::NoCatalogue) => json!({}),
                        None => json!({"configOptions": mode_and_effort_options(&value)}),
                    };
                    json!({"jsonrpc": "2.0", "id": id, "result": result})
                }
                "session/set_config_option" => {
                    let value = frame
                        .pointer("/params/value")
                        .and_then(Value::as_str)
                        .unwrap_or("medium")
                        .to_owned();
                    json!({"jsonrpc": "2.0", "id": id, "result": {
                        "configOptions": effort_options(&value),
                    }})
                }
                "session/prompt" => {
                    let session_id = frame
                        .pointer("/params/sessionId")
                        .and_then(Value::as_str)
                        .expect("prompt session");
                    self.prompt_sessions
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(session_id.to_owned());
                    json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}})
                }
                "session/load" => {
                    let restored = self
                        .restored_mode
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    let reply = match restored {
                        Some(mode) => json!({"jsonrpc": "2.0", "id": id, "result": {
                            "configOptions": mode_and_effort_options(&mode),
                        }}),
                        None => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    };
                    if self.hold_loads.load(Ordering::SeqCst) {
                        let held = self.clone();
                        let writer = Arc::clone(&writer);
                        tokio::spawn(async move {
                            held.loads_released_now().await;
                            let _sent = write_line(&writer, &reply).await;
                        });
                        continue;
                    }
                    reply
                }
                "session/close" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                _ => json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32601, "message": "method not found",
                }}),
            };
            let is_setting = method == "session/set_config_option";
            if is_setting && self.hold_settings.load(Ordering::SeqCst) {
                let held = self.clone();
                let writer = Arc::clone(&writer);
                tokio::spawn(async move {
                    held.settings_released_now().await;
                    if write_line(&writer, &reply).await {
                        held.record_setting_answer();
                    }
                });
                continue;
            }
            if !write_line(&writer, &reply).await {
                return;
            }
            if is_setting {
                self.record_setting_answer();
            }
        }
    }

    fn record_setting_answer(&self) {
        self.settings_answered.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    /// Resolves once load answers are no longer held.
    async fn loads_released_now(&self) {
        loop {
            let released = self.loads_released.notified();
            if !self.hold_loads.load(Ordering::SeqCst) {
                return;
            }
            released.await;
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

/// A mode select option currently `mode`, and the effort setting.
fn mode_and_effort_options(mode: &str) -> Value {
    let mut options = effort_options("medium");
    if let Some(options) = options.as_array_mut() {
        options.push(json!({
            "id": "mode", "name": "Mode", "category": "mode", "type": "select",
            "currentValue": mode,
            "options": [
                {"value": "default", "name": "Default"},
                {"value": "plan", "name": "Plan"},
                {"value": "auto", "name": "Auto"},
            ],
        }));
    }
    options
}

/// Writes one JSON-RPC line; false once the client end has closed.
async fn write_line(writer: &AgentWriter, reply: &Value) -> bool {
    let mut encoded = reply.to_string();
    encoded.push('\n');
    let mut writer = writer.lock().await;
    writer.write_all(encoded.as_bytes()).await.is_ok() && writer.flush().await.is_ok()
}
