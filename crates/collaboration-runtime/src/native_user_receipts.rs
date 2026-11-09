//! Connection-local correlation retained until the committed receipt is sent.
use serde_json::Value;
use session_event_model::InputId;
use std::collections::{HashMap, VecDeque};

struct PendingUserReceipt {
    thread_id: String,
    input_id: InputId,
    client_id: String,
    item_id: Option<String>,
}

pub(crate) struct NativeUserReceipts {
    outstanding: VecDeque<PendingUserReceipt>,
    accepted: HashMap<String, InputId>,
    capacity: usize,
}

impl Default for NativeUserReceipts {
    fn default() -> Self {
        Self {
            outstanding: VecDeque::new(),
            accepted: HashMap::new(),
            capacity: 128,
        }
    }
}

impl NativeUserReceipts {
    #[cfg(test)]
    pub(crate) fn with_capacity(capacity: std::num::NonZeroUsize) -> Self {
        Self {
            capacity: capacity.get(),
            ..Self::default()
        }
    }

    pub(crate) fn register(&mut self, request: &Value, input_id: InputId) {
        let (Some(thread_id), Some(client_id)) = (
            request.pointer("/params/threadId").and_then(Value::as_str),
            request
                .pointer("/params/clientUserMessageId")
                .and_then(Value::as_str),
        ) else {
            return;
        };
        if self.outstanding.len() == self.capacity
            && let Some(evicted) = self.outstanding.pop_front()
        {
            self.accepted
                .retain(|_, accepted_id| accepted_id != &evicted.input_id);
            tracing::warn!(
                capacity = self.capacity,
                "evicted oldest outstanding native user receipt correlation before completion"
            );
        }
        self.outstanding.push_back(PendingUserReceipt {
            thread_id: thread_id.to_owned(),
            input_id,
            client_id: client_id.to_owned(),
            item_id: None,
        });
    }

    pub(crate) fn refuse(&mut self, input_id: &InputId) {
        self.outstanding
            .retain(|receipt| &receipt.input_id != input_id);
        self.accepted
            .retain(|_, accepted_id| accepted_id != input_id);
    }

    pub(crate) fn accept(&mut self, thread_id: &str, input_id: &InputId) {
        self.accepted.remove(thread_id);
        if self
            .outstanding
            .iter()
            .any(|receipt| receipt.thread_id == thread_id && &receipt.input_id == input_id)
        {
            self.accepted.insert(thread_id.to_owned(), input_id.clone());
        }
    }

    /// Clear the next-item binding; outstanding input/item identities survive resets.
    pub(crate) fn complete_turn(&mut self, thread_id: &str) {
        self.accepted.remove(thread_id);
    }

    pub(crate) fn stamp(
        &mut self,
        thread_id: &str,
        item_id: &str,
        starts_item: bool,
        item: &mut Value,
        previous: Option<&Value>,
    ) {
        let existing = self.outstanding.iter().find(|receipt| {
            receipt.thread_id == thread_id && receipt.item_id.as_deref() == Some(item_id)
        });
        let client_id = if let Some(receipt) = existing {
            if self.accepted.get(thread_id) == Some(&receipt.input_id) {
                self.accepted.remove(thread_id);
            }
            Some(Value::String(receipt.client_id.clone()))
        } else if let Some(client_id) = previous.and_then(|item| item.get("clientId")) {
            Some(client_id.clone())
        } else if starts_item {
            self.accepted.remove(thread_id).and_then(|input_id| {
                let receipt = self.outstanding.iter_mut().find(|receipt| {
                    receipt.thread_id == thread_id && receipt.input_id == input_id
                })?;
                // A replay can re-accept an input, but can never assign it to a different item.
                if receipt.item_id.is_some() {
                    return None;
                }
                receipt.item_id = Some(item_id.to_owned());
                Some(Value::String(receipt.client_id.clone()))
            })
        } else {
            None
        };
        if let Some(client_id) = client_id {
            item["clientId"] = client_id;
        }
    }

    /// Call only after the frame has successfully been sent on this websocket.
    pub(crate) fn delivered(&mut self, frame: &Value) {
        if frame.get("method").and_then(Value::as_str) != Some("item/completed")
            || frame.pointer("/params/item/type").and_then(Value::as_str) != Some("userMessage")
        {
            return;
        }
        let (Some(thread_id), Some(item_id)) = (
            frame.pointer("/params/threadId").and_then(Value::as_str),
            frame.pointer("/params/item/id").and_then(Value::as_str),
        ) else {
            return;
        };
        self.outstanding.retain(|receipt| {
            receipt.thread_id != thread_id || receipt.item_id.as_deref() != Some(item_id)
        });
        self.accepted.retain(|accepted_thread, input_id| {
            self.outstanding.iter().any(|receipt| {
                &receipt.thread_id == accepted_thread && &receipt.input_id == input_id
            })
        });
    }
}
