//! Raw-row evidence keeps unexpected retained fields visible before identity assertions.
#![deny(clippy::too_many_lines)]
use super::{
    forwarded_post_lifecycle_oracles::POST_MARKER,
    forwarded_post_scenarios::{
        HOLDER_PEER, HOLDER_SERVICE, SENDER_PEER, SENDER_SERVICE, SENDER_SESSION,
    },
};
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_protocol::{ConnectionId, DatabaseId, FrameId};
use serde_json::{Value, json};

pub(super) fn records<'a>(state: &'a Value, field: &str) -> ConformanceResult<&'a Vec<Value>> {
    state
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("owning forward observation omitted {field}").into())
}

pub(super) fn no_body(state: &Value) -> ConformanceResult<()> {
    if [POST_MARKER, "transient-holder-decision"]
        .iter()
        .any(|marker| state.to_string().contains(marker))
    {
        return Err("the owning forward store retained opaque forwarding content".into());
    }
    Ok(())
}

pub(super) fn no_retained_state(sender: &Value, holder: &Value) -> ConformanceResult<()> {
    for state in [sender, holder] {
        no_body(state)?;
        if !records(state, "pending")?.is_empty() || !records(state, "delivered")?.is_empty() {
            return Err("terminal forwarding retained durable correlation state".into());
        }
    }
    Ok(())
}

pub(super) fn decoded_records(state: &Value, field: &str) -> ConformanceResult<Vec<Value>> {
    no_body(state)?;
    records(state, field)?
        .iter()
        .map(|row| {
            let key = row
                .get("key")
                .and_then(Value::as_str)
                .ok_or("owning record key absent")?;
            let raw = row
                .get("record")
                .and_then(Value::as_str)
                .ok_or("owning record JSON absent")?;
            let record: Value = serde_json::from_str(raw)?;
            if record.get("forwardFrame") != Some(&json!(key)) {
                return Err("owning correlation key differs from its raw stored record".into());
            }
            let _valid_frame = FrameId::try_from(key.to_owned())?;
            Ok(record)
        })
        .collect()
}

pub(super) fn pending_record(
    state: &Value,
    database: DatabaseId,
    request: FrameId,
    phase: Value,
) -> ConformanceResult<Value> {
    let pending = decoded_records(state, "pending")?;
    if pending.len() != 1 || !records(state, "delivered")?.is_empty() {
        return Err("sender owning state did not contain exactly its one pending request".into());
    }
    let row = pending
        .into_iter()
        .next()
        .ok_or("original pending row absent")?;
    if row.get("requestFrame") != Some(&json!(request))
        || row.get("project") != Some(&json!(database))
        || row.get("session") != Some(&sender_session())
        || row.get("phase") != Some(&phase)
    {
        return Err("pending row changed the actual request, project, session or phase".into());
    }
    let connection = row
        .get("connection")
        .and_then(Value::as_str)
        .ok_or("connection identity absent")?;
    let _valid_connection = ConnectionId::try_from(connection.to_owned())?;
    if !row
        .get("deadline")
        .and_then(Value::as_i64)
        .is_some_and(|deadline| deadline > 0)
    {
        return Err("pending row omitted the original absolute deadline".into());
    }
    Ok(row)
}

pub(super) fn sender_session() -> Value {
    json!({"peer":SENDER_PEER,"sessionId":SENDER_SESSION})
}

pub(super) fn live_records(
    sender: &Value,
    holder: &Value,
    database: DatabaseId,
    request: FrameId,
    frame: FrameId,
) -> ConformanceResult<()> {
    let pending = pending_record(
        sender,
        database,
        request,
        json!({"kind":"awaitingHolder","value":{"installation":HOLDER_SERVICE}}),
    )?;
    let delivered = decoded_records(holder, "delivered")?;
    if delivered.len() != 1 || !records(holder, "pending")?.is_empty() {
        return Err(
            "holder owning state did not contain exactly its actual delivered request".into(),
        );
    }
    let delivered = delivered.first().ok_or("delivered row absent")?;
    if pending.get("forwardFrame") != Some(&json!(frame))
        || delivered.get("forwardFrame") != Some(&json!(frame))
        || delivered.get("sender") != Some(&json!(SENDER_SERVICE))
        || delivered.get("holder") != Some(&json!(HOLDER_PEER))
        || delivered.get("project") != Some(&json!(database))
        || delivered.get("deadline") != pending.get("deadline")
    {
        return Err(
            "owning rows lost the actual frame, project, sender, holder or deadline".into(),
        );
    }
    Ok(())
}
