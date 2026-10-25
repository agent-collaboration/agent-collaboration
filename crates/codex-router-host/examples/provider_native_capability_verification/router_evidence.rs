//! Read-only evidence from the candidate Router: app-server requests on the endpoint
//! socket, bounded public `events observe`, Router operation rows and health.
use crate::bounded_process::run_bounded;
use crate::native_fixture::write_private;
use crate::verification_models::{
    NativeCapabilitiesPlan, ObservationCursor, OperationRow, ProviderObservation, RpcReply,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::{Connection, Row, sqlite::SqliteConnectOptions};
use std::{path::Path, time::Duration};
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

/// `agent-collaboration events observe` parser bounds
/// (crates/agent-collaboration/src/event_commands.rs:49-79): `--max-events` 1..=4096,
/// `--max-bytes` 1..=1_048_576, and `--after-sequence` requires `--epoch`.
pub(crate) const OBSERVE_MAX_EVENTS: u32 = 4096;
pub(crate) const OBSERVE_MAX_BYTES: u32 = 1_048_576;
pub(crate) const OBSERVE_MAX_PAGES: usize = 64;
const RPC_REPLY_SECONDS: u64 = 30;
const OBSERVE_PROCESS_SECONDS: u64 = 30;
/// One page's JSON: the event bytes `--max-bytes` admits plus its envelope.
const OBSERVE_OUTPUT_MAX_BYTES: usize = 4 * OBSERVE_MAX_BYTES as usize;
const OBSERVE_DIAGNOSTIC_MAX_BYTES: usize = 64 * 1024;
const HEALTH_MAX_BYTES: usize = 64 * 1024;

/// Send app-server requests as the installed client would, retaining raw replies.
pub(crate) async fn consumer_requests(
    plan: &NativeCapabilitiesPlan,
    requests: &[(&str, Value)],
    evidence: &Path,
    label: &str,
) -> Result<Vec<RpcReply>, String> {
    let stream = UnixStream::connect(&plan.endpoint_socket)
        .await
        .map_err(|error| format!("endpoint socket unreachable: {error}"))?;
    let config = WebSocketConfig::default().max_message_size(Some(16 * 1024 * 1024));
    let (mut socket, _) =
        tokio_tungstenite::client_async_with_config("ws://localhost/rpc", stream, Some(config))
            .await
            .map_err(|error| format!("app-server handshake failed: {error}"))?;
    let initialize =
        json!({"clientInfo": {"name": "native-capabilities-verifier", "version": "1"}});
    let mut replies = Vec::new();
    for (index, (method, params)) in std::iter::once(&("initialize", initialize))
        .chain(requests.iter())
        .enumerate()
    {
        let request_id = index as u64 + 1;
        socket
            .send(Message::text(
                json!({"id": request_id, "method": method, "params": params}).to_string(),
            ))
            .await
            .map_err(|error| format!("{method} send failed: {error}"))?;
        let text = loop {
            let frame = tokio::time::timeout(Duration::from_secs(RPC_REPLY_SECONDS), socket.next())
                .await
                .map_err(|_| format!("{method} reply timed out"))?
                .ok_or_else(|| format!("{method} socket closed"))?
                .map_err(|error| format!("{method} read failed: {error}"))?;
            if let Message::Text(text) = frame
                && serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|reply| reply.get("id").and_then(Value::as_u64))
                    == Some(request_id)
            {
                break text.to_string();
            }
        };
        write_private(
            &evidence.join(format!(
                "{label}-{request_id}-{}.json",
                method.replace('/', "-")
            )),
            text.as_bytes(),
        )?;
        if request_id > 1 {
            replies.push(
                serde_json::from_str(&text).map_err(|error| format!("{method} reply: {error}"))?,
            );
        }
    }
    let _ = socket.close(None).await;
    Ok(replies)
}

/// One `events observe` page within the CLI's parser bounds.
pub(crate) fn observe_arguments(
    plan: &NativeCapabilitiesPlan,
    cursor: Option<ObservationCursor>,
) -> Vec<String> {
    let mut arguments = vec![
        "events".to_owned(),
        "observe".to_owned(),
        "--endpoint".to_owned(),
        plan.endpoint_id.as_str().to_owned(),
        "--session".to_owned(),
        plan.session_id.clone(),
        "--attach".to_owned(),
        "--timeout-seconds".to_owned(),
        "2".to_owned(),
        "--max-events".to_owned(),
        OBSERVE_MAX_EVENTS.to_string(),
        "--max-bytes".to_owned(),
        OBSERVE_MAX_BYTES.to_string(),
        "--service-directory".to_owned(),
        plan.service_directory().display().to_string(),
    ];
    if let Some(cursor) = cursor {
        // The parser requires --epoch with --after-sequence; it is the observed epoch.
        arguments.extend([
            "--after-sequence".to_owned(),
            cursor.after_sequence.to_string(),
            "--epoch".to_owned(),
            cursor.epoch.to_string(),
        ]);
    }
    arguments
}

/// Follow result-limited pages until the deadline ends the observation, as the
/// client's own follower does (provider_session_observation.rs `accept`). An epoch
/// change, resync, disconnect or page overflow fails instead of returning a partial
/// history as complete.
pub(crate) fn paginate_observation(
    mut fetch: impl FnMut(Option<ObservationCursor>, usize) -> Result<ProviderObservation, String>,
    mut cursor: Option<ObservationCursor>,
) -> Result<ProviderObservation, String> {
    let mut events = Vec::new();
    for page in 0..OBSERVE_MAX_PAGES {
        let observed = fetch(cursor, page)?;
        if cursor.is_some_and(|cursor| observed.epoch != cursor.epoch) {
            return Err("observation epoch changed: retained history was reset".to_owned());
        }
        let next = observed.cursor();
        let finished = observed.end_reason == "deadlineReached";
        if !finished && (observed.end_reason != "resultLimitReached" || observed.events.is_empty())
        {
            return Err(format!(
                "observation ended with {}; resync is required",
                observed.end_reason
            ));
        }
        events.extend(observed.events.iter().cloned());
        if finished {
            return Ok(ProviderObservation { events, ..observed });
        }
        cursor = Some(next);
    }
    Err("observation did not complete within its page bound".to_owned())
}

/// A complete bounded public observation of the exact declared Session.
pub(crate) async fn observe_session(
    plan: &NativeCapabilitiesPlan,
    service_id: &str,
    evidence: &Path,
    label: &str,
    cursor: Option<ObservationCursor>,
) -> Result<ProviderObservation, String> {
    let mut pages = Vec::new();
    let mut page_cursor = cursor;
    for page in 0..OBSERVE_MAX_PAGES {
        let mut command = tokio::process::Command::new(&plan.binaries.agent_collaboration.path);
        command.args(observe_arguments(plan, page_cursor));
        let output = run_bounded(
            command,
            "public provider observation",
            Duration::from_secs(OBSERVE_PROCESS_SECONDS),
            OBSERVE_OUTPUT_MAX_BYTES,
            OBSERVE_DIAGNOSTIC_MAX_BYTES,
        )
        .await?;
        write_private(
            &evidence.join(format!("{label}-observation-{page}.json")),
            &output.stdout,
        )?;
        write_private(
            &evidence.join(format!("{label}-observation-{page}.stderr")),
            &output.stderr,
        )?;
        if !output.status.success() {
            return Err("public provider observation failed; raw response retained".to_owned());
        }
        let observed: ProviderObservation = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("observation does not decode: {error}"))?;
        if observed.target.session_id != plan.session_id
            || observed.target.endpoint.endpoint_id != plan.endpoint_id.as_str()
            || observed.target.endpoint.service_id != service_id
        {
            return Err("public observation returned a different Session".to_owned());
        }
        let done = observed.end_reason != "resultLimitReached";
        page_cursor = Some(observed.cursor());
        pages.push(observed);
        if done {
            break;
        }
    }
    let mut pages = pages.into_iter();
    paginate_observation(
        |_, _| {
            pages
                .next()
                .ok_or_else(|| "observation did not complete within its page bound".to_owned())
        },
        cursor,
    )
}

/// Router-owned durable operations for the Session, read in a read-only connection.
/// Schema: crates/collaboration-service/migrations/202609200001_provider_operations.sql.
pub(crate) async fn operation_rows(
    plan: &NativeCapabilitiesPlan,
    evidence: &Path,
    label: &str,
) -> Result<Vec<OperationRow>, String> {
    let options = SqliteConnectOptions::new()
        .filename(plan.service_directory().join("provider-operations.sqlite"))
        .read_only(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("operation store unreadable: {error}"))?;
    let rows = sqlx::query(
        "SELECT operation_id, operation_kind, stage, admitted_at_ms, terminal_at_ms \
         FROM provider_operations WHERE target_session_id = ? ORDER BY admitted_at_ms, operation_id",
    )
    .bind(&plan.session_id)
    .fetch_all(&mut connection)
    .await
    .map_err(|error| format!("operation rows unreadable: {error}"))?;
    let _ = connection.close().await;
    let operations = rows
        .iter()
        .map(|row| {
            Ok(OperationRow {
                operation_id: row.try_get("operation_id")?,
                operation_kind: row.try_get("operation_kind")?,
                stage: row.try_get("stage")?,
                admitted_at_ms: row.try_get("admitted_at_ms")?,
                terminal_at_ms: row.try_get("terminal_at_ms")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(|error| format!("operation row does not decode: {error}"))?;
    let json = serde_json::to_vec_pretty(&operations).map_err(|error| error.to_string())?;
    write_private(&evidence.join(format!("{label}-operations.json")), &json)?;
    Ok(operations)
}

/// The Router health version, as readiness only; never a credential oracle.
pub(crate) async fn router_health_version(port: u16) -> Result<String, String> {
    let mut response = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/healthz"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|error| format!("Router health unreachable: {error}"))?;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Router health unreadable: {error}"))?
    {
        if body.len() + chunk.len() > HEALTH_MAX_BYTES {
            return Err("Router health exceeds its bound".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let health: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("Router health does not decode: {error}"))?;
    health
        .get("binary_version")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Router health has no binary_version".to_owned())
}
