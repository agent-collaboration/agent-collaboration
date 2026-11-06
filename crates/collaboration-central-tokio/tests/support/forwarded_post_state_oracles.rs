//! Raw-row evidence keeps unexpected retained fields visible before identity assertions.
#![deny(clippy::too_many_lines)]
use super::{
    forwarded_post_lifecycle_oracles::POST_MARKER,
    forwarded_post_scenarios::{HOLDER_PEER, HOLDER_SERVICE, SENDER_PEER, SENDER_SESSION},
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

pub(super) fn typed_records(state: &Value, field: &str) -> ConformanceResult<Vec<Value>> {
    no_body(state)?;
    records(state, field)?
        .iter()
        .map(|row| {
            for column in [
                "id",
                "forwardFrameId",
                "requestFrameId",
                "originMachineId",
                "originSessionId",
                "connectionId",
                "projectId",
            ] {
                let value = row
                    .get(column)
                    .and_then(Value::as_str)
                    .ok_or("owning typed identity column absent")?;
                let identity = durable_steps::Uuid::parse_str(value)?;
                if identity.get_version_num() != 7 || identity.to_string() != value {
                    return Err(format!("owning {column} is not canonical UUIDv7").into());
                }
            }
            for column in ["createdAt", "updatedAt", "deadlineAt"] {
                let value = row
                    .get(column)
                    .and_then(Value::as_str)
                    .ok_or("owning integer column absent")?;
                let integer = value.parse::<i64>()?;
                if integer.to_string() != value {
                    return Err("noncanonical owning integer column".into());
                }
            }
            let side = if field == "pending" {
                "origin"
            } else {
                "holder"
            };
            if row.get("side") != Some(&json!(side)) {
                return Err("owning correlation has the wrong side".into());
            }
            Ok(row.clone())
        })
        .collect()
}

pub(super) fn pending_record(
    state: &Value,
    database: DatabaseId,
    request: FrameId,
    phase: Value,
) -> ConformanceResult<Value> {
    let pending = typed_records(state, "pending")?;
    if pending.len() != 1 {
        return Err("sender owning state did not contain exactly its one pending request".into());
    }
    let row = pending
        .into_iter()
        .next()
        .ok_or("original pending row absent")?;
    if row.get("requestFrameId") != Some(&json!(request))
        || row.get("projectId") != Some(&json!(database))
        || row.get("originMachineId") != Some(&json!(SENDER_PEER))
        || row.get("originSessionId") != Some(&json!(SENDER_SESSION))
        || row.get("phase")
            != Some(&json!(if phase.get("kind") == Some(&json!("routing")) {
                "routing"
            } else if phase
                == json!({"kind":"awaitingHolder","value":{"installation":HOLDER_SERVICE}})
            {
                "awaiting_holder"
            } else {
                return Err("unexpected oracle phase".into());
            }))
    {
        return Err("pending row changed the actual request, project, session or phase".into());
    }
    let connection = row
        .get("connectionId")
        .and_then(Value::as_str)
        .ok_or("connection identity absent")?;
    let _valid_connection = ConnectionId::try_from(connection.to_owned())?;
    if !row
        .get("deadlineAt")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
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
    let delivered = typed_records(holder, "delivered")?;
    if delivered.len() != 1 || records(holder, "pending")? != records(sender, "pending")? {
        return Err(
            "holder owning state did not contain exactly its actual delivered request".into(),
        );
    }
    let delivered = delivered.first().ok_or("delivered row absent")?;
    if pending.get("forwardFrameId") != Some(&json!(frame))
        || delivered.get("forwardFrameId") != Some(&json!(frame))
        || delivered.get("originMachineId") != Some(&json!(SENDER_PEER))
        || delivered.get("expectedHolderMachineId") != Some(&json!(HOLDER_PEER))
        || delivered.get("projectId") != Some(&json!(database))
        || delivered.get("deadlineAt") != pending.get("deadlineAt")
    {
        return Err(
            "owning rows lost the actual frame, project, sender, holder or deadline".into(),
        );
    }
    Ok(())
}
