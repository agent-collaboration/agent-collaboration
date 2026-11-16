//! The complete wire capture a fixture agent's shared transport writes
//! (`verification/fixtures/acp-stdio.ts`, `wireCapture`), recorded on a tap case.
//!
//! The fixture appends every stdio line it received or sent, in order, from `initialize` on.
//! Each line is recorded as a raw `WireFrame` on one connection before any case looks at it, with
//! N6's direction: a line the agent received is one N6 produced toward the agent, a line it sent
//! is one N6 consumed. Requests and notifications travel on their sender's family (N6's on
//! `AcpToAgent`, the agent's own on `AcpSessionUpdate`); a reply travels on its request's family,
//! the other way.

#![expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use std::{collections::HashMap, path::Path};

use codex_router_test_support::node_taps::{
    AcpPeer, EdgeParty, MessageFamily, RecordLayer, TapCase,
};
use serde_json::Value;

/// The connection every captured frame travels on: the agent's stdio.
const AGENT_STDIO: &str = "agent-stdio";

/// The connection setup every fixture session starts with: fixture setup, not a case's subject.
pub(crate) const SESSION_SETUP: &[&str] = &["initialize", "session/new"];

/// [`SESSION_SETUP`] plus a held turn: the prompt that holds it open and the cancel that ends it.
pub(crate) const HELD_TURN_SETUP: &[&str] = &[
    "initialize",
    "session/new",
    "session/prompt",
    "session/cancel",
];

/// Fields N6's ACP SDK generates per run: its request ids, and the temporary session directory.
const N6_GENERATED: &[&str] = &["id", "cwd"];

/// Which way a captured line crossed, from the fixture agent's side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentSide {
    /// The agent read the line: N6 produced it.
    Received,
    /// The agent wrote the line: N6 consumed it.
    Sent,
}

/// One captured line, parsed, exactly as it crossed.
#[derive(Clone, Debug)]
pub(crate) struct CapturedFrame {
    pub(crate) agent_side: AgentSide,
    pub(crate) frame: Value,
}

impl CapturedFrame {
    pub(crate) fn method(&self) -> Option<&str> {
        self.frame.get("method").and_then(Value::as_str)
    }
}

/// Reads the whole capture the fixture wrote, in order.
pub(crate) fn read_capture(path: &Path) -> Vec<CapturedFrame> {
    std::fs::read_to_string(path)
        .expect("wire capture")
        .lines()
        .map(|entry| {
            let entry: Value = serde_json::from_str(entry).expect("wire capture entry");
            let field = |name: &str| entry.get(name).and_then(Value::as_str);
            let agent_side = match field("direction") {
                Some("received") => Some(AgentSide::Received),
                Some("sent") => Some(AgentSide::Sent),
                _ => None,
            }
            .expect("wire capture direction is received or sent");
            let line = field("line").expect("wire capture line");
            CapturedFrame {
                agent_side,
                frame: serde_json::from_str(line).expect("captured line is JSON"),
            }
        })
        .collect()
}

/// The frames of `method` the agent received or sent on `side`, whole.
pub(crate) fn frames_of<'a>(
    frames: &'a [CapturedFrame],
    side: AgentSide,
    method: &str,
) -> Vec<&'a Value> {
    frames
        .iter()
        .filter(|captured| captured.agent_side == side && captured.method() == Some(method))
        .map(|captured| &captured.frame)
        .collect()
}

/// The replies, whole, that answer the requests of `method` sent from the other side of `side`.
pub(crate) fn replies_to<'a>(
    frames: &'a [CapturedFrame],
    side: AgentSide,
    method: &str,
) -> Vec<&'a Value> {
    let request_side = match side {
        AgentSide::Received => AgentSide::Sent,
        AgentSide::Sent => AgentSide::Received,
    };
    let request_ids: Vec<Option<&Value>> = frames_of(frames, request_side, method)
        .into_iter()
        .map(|request| request.get("id"))
        .collect();
    frames
        .iter()
        .filter(|captured| {
            captured.agent_side == side
                && captured.method().is_none()
                && request_ids.contains(&captured.frame.get("id"))
        })
        .map(|captured| &captured.frame)
        .collect()
}

/// Names a reply's record kind from the method it answers and the whole reply; `None` keeps the
/// default `response:<method>`.
pub(crate) type ReplyKind<'a> = &'a dyn Fn(&str, &Value) -> Option<String>;

/// Records every one of `frames` on `case`, in order, on the agent's stdio connection, and
/// reconciles the count: exactly one wire record per captured line. `after_each` runs after each
/// frame is recorded, so a case can record what the frame caused inside the Router at its place.
/// Frames of a method in `setup_methods`, and the replies to them, are fixture setup: checked and
/// counted like every frame, but outside the case's own view the catalog row compares.
/// `peer` is the ACP agent the fixture stands in for, which decides who owns the connection's
/// extension methods; `None` when the case exchanges only standard ACP.
pub(crate) fn record_frames(
    case: &mut TapCase,
    peer: Option<AcpPeer>,
    frames: &[CapturedFrame],
    setup_methods: &[&str],
    reply_kind: ReplyKind<'_>,
    after_each: &mut dyn FnMut(&mut TapCase, &CapturedFrame),
) {
    let records_before = case.records().len();
    case.connection(AGENT_STDIO);
    if let Some(peer) = peer {
        case.acp_peer(peer);
    }
    // Request ids to methods, by who sent the request.
    let mut sent_by_n6: HashMap<String, String> = HashMap::new();
    let mut sent_by_agent: HashMap<String, String> = HashMap::new();
    for captured in frames {
        let raw = captured.frame.clone();
        let id_key = raw.get("id").map(Value::to_string);
        let subject = match (captured.agent_side, captured.method()) {
            (AgentSide::Received, Some(method)) => {
                if let Some(id) = id_key {
                    sent_by_n6.insert(id, method.to_owned());
                }
                case.produced_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpToAgent,
                    EdgeParty::AcpAgent,
                    method,
                    raw,
                    N6_GENERATED,
                );
                method.to_owned()
            }
            (AgentSide::Sent, Some(method)) => {
                if let Some(id) = id_key {
                    sent_by_agent.insert(id, method.to_owned());
                }
                case.consumed_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpSessionUpdate,
                    EdgeParty::AcpAgent,
                    method,
                    raw,
                    &[],
                );
                method.to_owned()
            }
            // A reply N6 sent: it answers one of the agent's requests, whose ids the agent chose.
            (AgentSide::Received, None) => {
                let method = answered(&sent_by_agent, id_key.as_deref());
                let kind =
                    reply_kind(&method, &raw).unwrap_or_else(|| format!("response:{method}"));
                case.produced_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpSessionUpdate,
                    EdgeParty::AcpAgent,
                    &kind,
                    raw,
                    &[],
                );
                method
            }
            // A reply the agent sent: it answers one of N6's requests, whose ids the SDK minted.
            (AgentSide::Sent, None) => {
                let method = answered(&sent_by_n6, id_key.as_deref());
                let kind =
                    reply_kind(&method, &raw).unwrap_or_else(|| format!("response:{method}"));
                case.consumed_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpToAgent,
                    EdgeParty::AcpAgent,
                    &kind,
                    raw,
                    N6_GENERATED,
                );
                method
            }
        };
        if setup_methods.contains(&subject.as_str()) {
            case.mark_last_setup();
        }
        after_each(case, captured);
    }
    let recorded = case
        .records()
        .iter()
        .skip(records_before)
        .filter(|record| record.layer == RecordLayer::WireFrame)
        .count();
    assert_eq!(
        recorded,
        frames.len(),
        "every captured line is recorded once"
    );
}

/// The method a reply answers, or `unpaired` when no captured request has its id (the checker
/// then reports the orphan).
fn answered(requests: &HashMap<String, String>, id: Option<&str>) -> String {
    id.and_then(|id| requests.get(id))
        .cloned()
        .unwrap_or_else(|| "unpaired".to_owned())
}
