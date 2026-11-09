//! Connection-local correlation from native input IDs to committed user items.
use message_board::SessionRef;
use serde_json::Value;
use session_event_model::InputId;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct NativeUserReceipts {
    submitted: HashMap<(String, InputId), String>,
    accepted: HashMap<SessionRef, String>,
}

impl NativeUserReceipts {
    pub(crate) fn register(&mut self, request: &Value, input_id: InputId) {
        if let (Some(thread_id), Some(client_id)) = (
            request.pointer("/params/threadId").and_then(Value::as_str),
            request
                .pointer("/params/clientUserMessageId")
                .and_then(Value::as_str),
        ) {
            self.submitted
                .insert((thread_id.to_owned(), input_id), client_id.to_owned());
        }
    }

    pub(crate) fn refuse(&mut self, input_id: &InputId) {
        self.submitted
            .retain(|(_, submitted_id), _| submitted_id != input_id);
    }

    pub(super) fn accept(&mut self, session: &SessionRef, thread_id: &str, input_id: &InputId) {
        // Every accepted input supersedes the previous next-item binding, even
        // when it came from another client and has no connection-local ID.
        self.accepted.remove(session);
        if let Some(client_id) = self
            .submitted
            .remove(&(thread_id.to_owned(), input_id.clone()))
        {
            self.accepted.insert(session.clone(), client_id);
        }
    }

    pub(super) fn complete_turn(&mut self, session: &SessionRef) {
        self.accepted.remove(session);
    }

    pub(super) fn stamp(
        &mut self,
        session: &SessionRef,
        item: &mut Value,
        previous: Option<&Value>,
    ) {
        if let Some(client_id) = previous
            .and_then(|item| item.get("clientId"))
            .cloned()
            .or_else(|| self.accepted.remove(session).map(Value::String))
        {
            item["clientId"] = client_id;
        }
    }
}
