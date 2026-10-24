//! Read-only observation of the Router delivery queue for the declared Session, through
//! the Host's existing private ACP channel (`<service>/codex-acp.sock`, JSON lines) and
//! its public `_session/queue/list` (crates/collaboration-service/src/
//! provider_acp_session_route.rs:628-646). The route requires an actor at initialize
//! but `queue_list` does not authorize it; the verifier presents only its own Session.
//! Nothing here adds, cancels or reorders queued input.
use crate::native_fixture::write_private;
use crate::verification_models::{NativeCapabilitiesPlan, ObserverIdentity};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const QUEUE_LIST_METHOD: &str = "_session/queue/list";
const QUEUE_REPLY_DEADLINE: Duration = Duration::from_secs(30);
const QUEUE_FRAME_MAX_BYTES: usize = 1024 * 1024;

/// Queued input IDs, in queue order, from one `_session/queue/list` reply. An error
/// reply is an access limitation, never an empty queue.
pub(crate) fn queued_input_ids(reply: &Value) -> Result<Vec<String>, String> {
    if let Some(error) = reply.get("error") {
        return Err(format!("Router queue unobservable: {error}"));
    }
    reply
        .pointer("/result/items")
        .and_then(Value::as_array)
        .ok_or("Router queue reply has no items")?
        .iter()
        .map(|item| {
            item.get("inputId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "queued item has no inputId".to_owned())
        })
        .collect()
}

/// One newline-terminated frame, refusing any frame past the byte bound.
async fn read_frame(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Result<Value, String> {
    let mut frame = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| format!("ACP channel read failed: {error}"))?;
        if available.is_empty() {
            return Err("ACP channel closed before a reply".to_owned());
        }
        let (taken, complete) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        frame.extend_from_slice(available.get(..taken).unwrap_or_default());
        reader.consume(taken);
        if frame.len() > QUEUE_FRAME_MAX_BYTES {
            return Err("ACP frame exceeds its bound".to_owned());
        }
        if complete {
            return serde_json::from_slice(&frame)
                .map_err(|error| format!("ACP frame does not decode: {error}"));
        }
    }
}

/// The reply carrying `id`, skipping notifications in between.
async fn reply_for(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    id: u64,
) -> Result<Value, String> {
    loop {
        let frame = read_frame(reader).await?;
        if frame.get("id").and_then(Value::as_u64) == Some(id) {
            return Ok(frame);
        }
    }
}

/// The declared Session's queued input IDs, with the raw reply kept as
/// `{label}-queue-list.json`.
pub(crate) async fn observe_queue(
    plan: &NativeCapabilitiesPlan,
    service_id: &str,
    evidence: &Path,
    label: &str,
) -> Result<Vec<String>, String> {
    let actor: &ObserverIdentity = plan
        .queue_observer
        .as_ref()
        .ok_or("Router queue observation needs the verifier's own Session identity")?;
    let socket = plan.service_directory().join("codex-acp.sock");
    let exchange = async {
        let stream = UnixStream::connect(&socket)
            .await
            .map_err(|error| format!("ACP channel unreachable: {error}"))?;
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let session_ref = json!({
            "endpoint": {"serviceId": service_id, "endpointId": plan.endpoint_id.as_str()},
            "sessionId": plan.session_id,
        });
        let requests = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": 1, "clientCapabilities": {},
                "_meta": {"router": {"actor": actor}},
            }}),
            json!({"jsonrpc": "2.0", "id": 2, "method": QUEUE_LIST_METHOD, "params": {
                "sessionId": plan.session_id,
                "_meta": {"router": {"sessionRef": session_ref}},
            }}),
        ];
        let mut replies = Vec::new();
        for (id, request) in (1_u64..).zip(requests) {
            write
                .write_all(format!("{request}\n").as_bytes())
                .await
                .map_err(|error| format!("ACP channel write failed: {error}"))?;
            replies.push(reply_for(&mut reader, id).await?);
        }
        Ok::<_, String>(replies)
    };
    let replies = tokio::time::timeout(QUEUE_REPLY_DEADLINE, exchange)
        .await
        .map_err(|_| "Router queue observation timed out".to_owned())??;
    let [initialized, listed] = replies.as_slice() else {
        return Err("Router queue observation returned a different reply count".to_owned());
    };
    write_private(
        &evidence.join(format!("{label}-queue-list.json")),
        &serde_json::to_vec_pretty(&json!({"initialize": initialized, "queueList": listed}))
            .map_err(|error| error.to_string())?,
    )?;
    if initialized.get("result").is_none() {
        return Err("ACP channel refused initialization; Router queue unobservable".to_owned());
    }
    queued_input_ids(listed)
}
