//! Live recordings (D0 Program Design Y3): one JSON-lines format for every outside edge, checked
//! by the same edge checker and connection ledger the part tests use. The `check_recording`
//! example is its command-line entry point.
//!
//! Each line is one frame, `{"seq", "connection", "direction", "message"}`, with `direction`
//! `clientToServer` / `serverToClient`, or the Codex wire tee's `cliToServer` / `serverToCli`.
//! A line the privacy filter dropped is `{"seq", "connection", "direction", "redacted": {...}}`.
//! A recording is corrupt when a line does not parse, when `seq` is not 0, 1, 2, … in order, or
//! when it holds no `initialize` exchange; a corrupt recording is rejected whole, never partly
//! counted.

use super::json_path::at;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::connection_ledger::{ConnectionId, ConnectionLedger};
use super::edge_conformance::{
    CheckInput, EdgeConformance, EdgeDefinitions, OutsideEdge, OwnedCodecs, ProtocolRole,
    RecordLayer,
};

pub fn parse_edge(name: &str) -> Option<OutsideEdge> {
    Some(match name {
        "cli-codex" => OutsideEdge::CliCodex,
        "managed-codex" => OutsideEdge::ManagedCodex,
        "acp-agent" => OutsideEdge::AcpAgent,
        "acp-served-n8" => OutsideEdge::AcpServedN8,
        "acp-served-n9" => OutsideEdge::AcpServedN9,
        "mcp" => OutsideEdge::Mcp,
        _ => return None,
    })
}

fn sender_of(direction: &str) -> Option<ProtocolRole> {
    match direction {
        "clientToServer" | "cliToServer" => Some(ProtocolRole::Client),
        "serverToClient" | "serverToCli" => Some(ProtocolRole::Server),
        _ => None,
    }
}

/// One parsed line of a recording.
pub(super) enum Line {
    Frame {
        connection: String,
        direction: String,
        sender: ProtocolRole,
        message: Value,
    },
    Redacted {
        connection: String,
        direction: String,
        redacted: Value,
    },
}

/// Parses and validates the recording, or names why it is corrupt.
pub(super) fn read_recording(text: &str) -> Result<Vec<(u64, Line)>, String> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("line {}: does not parse: {error}", index + 1))?;
        let seq = at(&value, &["seq"])
            .as_u64()
            .ok_or_else(|| format!("line {}: no seq", index + 1))?;
        if seq != index as u64 {
            return Err(format!(
                "line {}: seq {seq} where {index} was due (a gap or reorder)",
                index + 1
            ));
        }
        let connection = match &at(&value, &["connection"]) {
            Value::String(name) => name.clone(),
            Value::Number(number) => number.to_string(),
            _ => return Err(format!("line {}: no connection", index + 1)),
        };
        let direction = at(&value, &["direction"])
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let sender = sender_of(&direction)
            .ok_or_else(|| format!("line {}: unknown direction {direction:?}", index + 1))?;
        let parsed = match (value.get("message"), value.get("redacted")) {
            (Some(message), None) => Line::Frame {
                connection,
                direction,
                sender,
                message: message.clone(),
            },
            (None, Some(redacted)) => Line::Redacted {
                connection,
                direction,
                redacted: redacted.clone(),
            },
            _ => {
                return Err(format!(
                    "line {}: needs exactly one of message or redacted",
                    index + 1
                ));
            }
        };
        lines.push((seq, parsed));
    }
    let initialized = lines.iter().any(|(_, line)| {
        matches!(line, Line::Frame { message, .. } if at(message, &["method"]) == "initialize")
    });
    if !initialized {
        return Err("the recording holds no initialize exchange".to_owned());
    }
    Ok(lines)
}

fn result_label(conformance: &EdgeConformance) -> &'static str {
    match conformance {
        EdgeConformance::Conforms { .. } => "conforms",
        EdgeConformance::Violations { .. } => "violations",
        EdgeConformance::Incomplete { .. } => "incomplete",
        EdgeConformance::NoAuthoritativeDefinition { .. } => "noAuthoritativeDefinition",
        EdgeConformance::NotSchemaChecked { .. } => "notSchemaChecked",
        // The worst of its parts: any violation, else any missing check, else conforms.
        EdgeConformance::Composite { .. } => {
            let leaves: Vec<&EdgeConformance> = conformance.leaves().collect();
            if leaves
                .iter()
                .any(|leaf| matches!(leaf, EdgeConformance::Violations { .. }))
            {
                "violations"
            } else if leaves
                .iter()
                .any(|leaf| matches!(leaf, EdgeConformance::Incomplete { .. }))
            {
                "incomplete"
            } else {
                "conforms"
            }
        }
    }
}

/// The tool names an MCP server's `tools/list` reply advertises: only on the MCP edge, only from
/// the server, and only a reply the checker paired with a `tools/list` request (its standard
/// verdict names `tools/list result`). Correction 4, V4: N11's true MCP definition for R53.
fn advertised_tools(
    edge: OutsideEdge,
    sender: ProtocolRole,
    conformance: &EdgeConformance,
    message: &Value,
) -> Vec<String> {
    let paired = conformance.leaves().next().is_some_and(|primary| {
        matches!(primary,
            EdgeConformance::Conforms { definition, .. }
            | EdgeConformance::Violations { definition, .. } if definition == "tools/list result")
    });
    if edge != OutsideEdge::Mcp || sender != ProtocolRole::Server || !paired {
        return Vec::new();
    }
    message
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Checks a whole recording of `edge`, returning the results the schema-audit report reads.
///
/// `codecs` are the owning checks of Router-owned contracts: the same registry the part tests use
/// (`OwnedCodecs::from_owner_registry` over `acp_provider_adapters::owned_contract_checks`, passed in by
/// `acp-provider-adapters/examples/check_recording.rs`), since test support never depends on the
/// parts it checks.
///
/// # Errors
/// When the recording is corrupt, naming why.
pub fn check_recording(
    definitions: &EdgeDefinitions,
    edge: OutsideEdge,
    recording: &str,
    text: &str,
    codecs: &OwnedCodecs,
) -> Result<Value, String> {
    let lines = read_recording(text)
        .map_err(|problem| format!("corrupt recording {recording}: {problem}"))?;
    let mut ledger = ConnectionLedger::default();
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut results = Vec::new();
    let mut redacted = Vec::new();
    for (seq, line) in &lines {
        match line {
            Line::Frame {
                connection,
                direction,
                sender,
                message,
            } => {
                let connection = ConnectionId(connection.clone());
                let conformance = definitions.check(
                    &CheckInput {
                        edge,
                        connection: &connection,
                        sender: *sender,
                        layer: &RecordLayer::WireFrame,
                        answers: None,
                        raw: message,
                        codecs,
                    },
                    &mut ledger,
                );
                *counts.entry(result_label(&conformance)).or_default() += 1;
                let tool = message
                    .pointer("/params/name")
                    .filter(|_| message.get("method") == Some(&json!("tools/call")));
                results.push(json!({"seq": seq, "connection": connection.0, "direction": direction,
                    "method": message.get("method"), "tool": tool,
                    "advertisedTools": advertised_tools(edge, *sender, &conformance, message),
                    "conformance": conformance}));
            }
            Line::Redacted {
                connection,
                direction,
                redacted: what,
            } => redacted.push(json!({"seq": seq, "connection": connection, "direction": direction, "redacted": what})),
        }
    }
    Ok(json!({
        "edge": edge,
        "recording": recording,
        "frames": lines.len(),
        "checked": results.len(),
        "redacted": redacted,
        "counts": counts,
        "unanswered": ledger.unanswered(),
        "results": results,
    }))
}

#[cfg(test)]
#[path = "live_recording_test.rs"]
mod tests;
