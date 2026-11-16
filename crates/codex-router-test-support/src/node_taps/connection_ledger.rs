//! The connection ledger (D0 Program Design Y2, "Pairing replies"): one per recording (a part-test
//! case, or a live recording file). It keys pending requests by connection, sending role and id,
//! so equal ids on two connections, or in opposite directions, never cross. It also keeps what
//! each connection negotiated in its `initialize` exchange and the MCP tools it advertised.

use super::json_path::at;
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::edge_conformance::ProtocolRole;

/// One recorded connection, named by the test or the live recorder.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ConnectionId(pub String);

impl ConnectionId {
    /// The connection a case records on when it names none.
    #[must_use]
    pub fn case_default() -> Self {
        Self("case".to_owned())
    }
}

/// The ACP agent on a connection, which decides who owns its extension methods (R52: owner by
/// edge plus method). A part test declares it; a live recording learns it from the agent's
/// negotiated `initialize` result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpPeer {
    Claude,
    Cursor,
}

/// The `agentInfo.name` Claude's ACP adapter reports (dist/acp-agent.js:947-951).
const CLAUDE_AGENT_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";

/// A request sent with the id of a request still waiting on the same connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateOutstandingId {
    pub id: Value,
    pub first_method: String,
}

/// A request still waiting for its reply when the recording ends.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnansweredRequest {
    pub connection: ConnectionId,
    pub id: String,
    pub method: String,
}

#[derive(Debug, Default)]
pub struct ConnectionLedger {
    /// `(connection, sender of the request, id key)` to the request's method and its id as it
    /// crossed (JSON text), so reports name the id the frame carried.
    pending: BTreeMap<(ConnectionId, RoleKey, String), (String, String)>,
    /// Each connection's `initialize` result, kept for negotiated-capability support.
    initialized: BTreeMap<ConnectionId, Value>,
    /// Each connection's advertised MCP tools, by name.
    tools: BTreeMap<ConnectionId, BTreeMap<String, Value>>,
    /// Each ACP connection's agent, declared or negotiated.
    peers: BTreeMap<ConnectionId, AcpPeer>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RoleKey {
    Client,
    Server,
}

impl From<ProtocolRole> for RoleKey {
    fn from(role: ProtocolRole) -> Self {
        match role {
            ProtocolRole::Client => Self::Client,
            ProtocolRole::Server => Self::Server,
        }
    }
}

/// A JSON-RPC id as the ledger keys it (correction 4, V6). A number is keyed by its numeric value,
/// so `1` and `1.0` are one id; a string by its text, so `1` and `"1"` stay distinct.
fn id_key(id: &Value) -> String {
    match id {
        Value::Number(number) => match (number.as_i64(), number.as_u64(), number.as_f64()) {
            (Some(integer), _, _) => format!("n:{integer}"),
            (None, Some(integer), _) => format!("n:{integer}"),
            // The whole signed 64-bit range: [-2^63, 2^63). -2^63 is exactly representable as an
            // f64; 2^63 is not in range (correction 5, C4-3).
            (None, None, Some(float))
                if float.fract() == 0.0
                    && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0)
                        .contains(&float) =>
            {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "An integral float inside the i64 range converts to i64 exactly."
                )]
                let integer = float as i64;
                format!("n:{integer}")
            }
            _ => format!("n:{number}"),
        },
        other => other.to_string(),
    }
}

impl ConnectionLedger {
    /// Opens a request `sender` sent with `id`. A request reusing the id of one still waiting
    /// on the same connection breaks JSON-RPC: the first pairing is kept and its method returned,
    /// so the caller reports the duplicate instead of silently re-pairing the first reply.
    pub fn open_request(
        &mut self,
        connection: &ConnectionId,
        sender: ProtocolRole,
        id: &Value,
        method: &str,
    ) -> Result<(), DuplicateOutstandingId> {
        let key = (connection.clone(), sender.into(), id_key(id));
        if let Some((first, _)) = self.pending.get(&key) {
            return Err(DuplicateOutstandingId {
                id: id.clone(),
                first_method: first.clone(),
            });
        }
        self.pending
            .insert(key, (method.to_owned(), id.to_string()));
        Ok(())
    }

    /// Closes the request a reply from `replier` with `id` answers, returning its method.
    pub fn close_reply(
        &mut self,
        connection: &ConnectionId,
        replier: ProtocolRole,
        id: &Value,
    ) -> Option<String> {
        self.pending
            .remove(&(connection.clone(), replier.other().into(), id_key(id)))
            .map(|(method, _)| method)
    }

    pub fn record_initialize_result(&mut self, connection: &ConnectionId, result: &Value) {
        let agent = result
            .get("agentInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str);
        if agent == Some(CLAUDE_AGENT_PACKAGE) {
            self.peers
                .entry(connection.clone())
                .or_insert(AcpPeer::Claude);
        }
        self.initialized.insert(connection.clone(), result.clone());
    }

    /// Declares which agent a connection talks to; a declaration wins over negotiation.
    pub fn declare_peer(&mut self, connection: &ConnectionId, peer: AcpPeer) {
        self.peers.insert(connection.clone(), peer);
    }

    /// The agent on this connection, if declared or negotiated.
    #[must_use]
    pub fn peer(&self, connection: &ConnectionId) -> Option<AcpPeer> {
        self.peers.get(connection).copied()
    }

    /// The `initialize` result this connection negotiated, if its capture held one.
    #[must_use]
    pub fn negotiated(&self, connection: &ConnectionId) -> Option<&Value> {
        self.initialized.get(connection)
    }

    pub fn record_tools(&mut self, connection: &ConnectionId, tools: &[Value]) {
        let advertised = self.tools.entry(connection.clone()).or_default();
        for tool in tools {
            if let Some(name) = at(tool, &["name"]).as_str() {
                advertised.insert(name.to_owned(), tool.clone());
            }
        }
    }

    /// The tool `name` as this connection's `tools/list` advertised it.
    #[must_use]
    pub fn tool(&self, connection: &ConnectionId, name: &str) -> Option<&Value> {
        self.tools.get(connection)?.get(name)
    }

    /// Every request still open, in key order.
    #[must_use]
    pub fn unanswered(&self) -> Vec<UnansweredRequest> {
        self.pending
            .iter()
            .map(|((connection, _, _), (method, id))| UnansweredRequest {
                connection: connection.clone(),
                id: id.clone(),
                method: method.clone(),
            })
            .collect()
    }
}
