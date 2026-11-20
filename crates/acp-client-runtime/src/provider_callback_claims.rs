//! Claim inbound provider requests before the ACP SDK allocates response slots.

use agent_client_protocol::schema::v1::RequestId;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, MutexGuard, PoisonError},
};

/// One connection owns these claims. A completed write releases its exact request.
#[derive(Default)]
pub(crate) struct ProviderCallbackClaims(Mutex<ActiveCallbacks>);

#[derive(Default)]
struct ActiveCallbacks {
    by_peer: BTreeSet<(RequestId, String)>,
    by_sdk: BTreeMap<RequestId, (RequestId, String)>,
}

impl ProviderCallbackClaims {
    fn active(&self) -> MutexGuard<'_, ActiveCallbacks> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn incoming(&self, frame: String) -> Option<String> {
        let Ok(mut value) = serde_json::from_str::<Value>(&frame) else {
            return Some(frame);
        };
        let mut active = self.active();
        let mut changed = false;
        if let Some(entries) = value.as_array_mut() {
            if entries.is_empty() {
                return Some(frame);
            }
            entries.retain_mut(|entry| active.retain_request(entry, &mut changed));
            if entries.is_empty() {
                return None;
            }
        } else if !active.retain_request(&mut value, &mut changed) {
            return None;
        }
        Some(if changed { value.to_string() } else { frame })
    }

    pub(crate) fn outgoing(&self, frame: String) -> (String, Vec<RequestId>) {
        let active = self.active();
        if active.by_sdk.is_empty() {
            return (frame, Vec::new());
        }
        let Ok(mut value) = serde_json::from_str::<Value>(&frame) else {
            return (frame, Vec::new());
        };
        let mut changed = false;
        let mut replies = Vec::new();
        let entries = match &mut value {
            Value::Array(entries) => entries.as_mut_slice(),
            single => std::slice::from_mut(single),
        };
        for response in entries.iter_mut().filter_map(Value::as_object_mut) {
            if response.contains_key("method")
                || !(response.contains_key("result") || response.contains_key("error"))
            {
                continue;
            }
            if let Some(sdk_id) = request_id(response)
                && let Some((peer_id, _)) = active.by_sdk.get(&sdk_id)
            {
                if &sdk_id != peer_id {
                    response.insert("id".to_owned(), id_value(peer_id));
                    changed = true;
                }
                replies.push(sdk_id);
            }
        }
        (if changed { value.to_string() } else { frame }, replies)
    }

    pub(crate) fn flushed(&self, replies: Vec<RequestId>) {
        if replies.is_empty() {
            return;
        }
        let mut active = self.active();
        for id in replies {
            if let Some(claim) = active.by_sdk.remove(&id) {
                active.by_peer.remove(&claim);
            }
        }
    }
}

impl ActiveCallbacks {
    fn retain_request(&mut self, entry: &mut Value, changed: &mut bool) -> bool {
        let Some(request) = entry.as_object_mut() else {
            return true;
        };
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return true;
        };
        let Some(peer_id) = request_id(request) else {
            return true;
        };
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || request.get("params").is_some_and(|params| {
                !(params.is_object() || params.is_array() || params.is_null())
            })
        {
            return true;
        }
        let claim = (peer_id.clone(), method.to_owned());
        if !self.by_peer.insert(claim.clone()) {
            *changed = true;
            return false;
        }
        // Responses do not carry the method. When different in-flight methods
        // reuse one wire ID, give the SDK a second ID to distinguish their replies.
        let sdk_id = if peer_id == RequestId::Null || self.by_sdk.contains_key(&peer_id) {
            let mut candidate = RequestId::Str(uuid::Uuid::now_v7().to_string());
            while self.by_sdk.contains_key(&candidate) {
                candidate = RequestId::Str(uuid::Uuid::now_v7().to_string());
            }
            request.insert("id".to_owned(), id_value(&candidate));
            *changed = true;
            candidate
        } else {
            peer_id
        };
        self.by_sdk.insert(sdk_id, claim);
        true
    }
}

fn request_id(message: &Map<String, Value>) -> Option<RequestId> {
    match message.get("id")? {
        Value::Null => Some(RequestId::Null),
        Value::Number(number) => number.as_i64().map(RequestId::Number),
        Value::String(text) => Some(RequestId::Str(text.clone())),
        _ => None,
    }
}

fn id_value(id: &RequestId) -> Value {
    match id {
        RequestId::Null => Value::Null,
        RequestId::Number(number) => Value::from(*number),
        RequestId::Str(text) => Value::String(text.clone()),
    }
}
