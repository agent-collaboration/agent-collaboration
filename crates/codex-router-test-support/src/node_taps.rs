//! Node taps: what one protocol node consumed and produced for one input.
//!
//! The protocol-contracts Program Design describes the Router as twelve nodes
//! (N1–N12) joined by typed edges. Its conversion catalog lists, per input
//! variant, the outputs a node may emit and the state it owns; its
//! producer/consumer table lists which node produces and consumes each message
//! family. A tap test drives one node in-process with one catalog input,
//! records each message at the node's edges plus the declared state before and
//! after, and compares the recording with the catalog row.
//!
//! A case whose row describes a later deliverable's change, or a current defect,
//! sits on [`expected_failures::EXPECTED_FAILURES`]. Such a case must keep
//! failing until its deliverable lands; an unexpected pass fails the test and
//! asks for the entry to be removed.
//!
//! When `CODEX_ROUTER_NODE_TAP_DIR` names a directory, every finished case also
//! writes `<dir>/<node>/<case>.json`; the producer/consumer report in
//! `verification/tooling/` reads those files.

pub mod connection_ledger;
pub mod edge_conformance;
pub mod expected_failures;
pub mod json_path;
pub mod live_recording;
mod recording;
pub mod schema_settlement;

use std::{collections::BTreeMap, fmt, path::PathBuf};

use serde::Serialize;

pub use connection_ledger::{AcpPeer, ConnectionId, ConnectionLedger};
pub use edge_conformance::{
    BodyRoot, CheckInput, ContractOwner, EdgeConformance, EdgeDefinitions, OutsideEdge, OwnedCodec,
    OwnedCodecs, OwnedVerdicts, ProtocolRole, RecordLayer, RouterContract, Support, Violation,
};
pub use expected_failures::{
    BehaviorDefect, Deliverable, EXPECTED_FAILURES, ExpectedFailure, RecordKey, SchemaDefect,
};
pub use schema_settlement::{RecordIntent, SchemaSettlement};

/// Environment variable naming the directory that receives tap recordings.
pub const NODE_TAP_DIR_ENV: &str = "CODEX_ROUTER_NODE_TAP_DIR";

/// A converting node of the protocol-contracts graph. N2, the opaque byte
/// relay, has no taps by design.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ProtocolNode {
    #[serde(rename = "N1")]
    N1CliFace,
    #[serde(rename = "N3")]
    N3OutputMapper,
    #[serde(rename = "N4")]
    N4CommandPort,
    #[serde(rename = "N5")]
    N5EventHub,
    #[serde(rename = "N6")]
    N6AcpClient,
    #[serde(rename = "N7")]
    N7ApprovalsBroker,
    #[serde(rename = "N8")]
    N8CodexAcpAdapter,
    #[serde(rename = "N9")]
    N9ProviderAcpRoute,
    #[serde(rename = "N10")]
    N10CodexClient,
    #[serde(rename = "N11")]
    N11McpServer,
    #[serde(rename = "N12")]
    N12ClaudeMetadata,
}

impl ProtocolNode {
    /// The short node label used in the Program Design (`N1` … `N12`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::N1CliFace => "N1",
            Self::N3OutputMapper => "N3",
            Self::N4CommandPort => "N4",
            Self::N5EventHub => "N5",
            Self::N6AcpClient => "N6",
            Self::N7ApprovalsBroker => "N7",
            Self::N8CodexAcpAdapter => "N8",
            Self::N9ProviderAcpRoute => "N9",
            Self::N10CodexClient => "N10",
            Self::N11McpServer => "N11",
            Self::N12ClaudeMetadata => "N12",
        }
    }
}

/// The other end of a tapped edge: another node or a party outside the Router.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "node")]
pub enum EdgeParty {
    Node(ProtocolNode),
    CodexCli,
    ManagedCodex,
    /// Claude or Cursor behind the ACP client.
    AcpAgent,
    /// MCP callers, the MCP carrier and CLI callers of the Router.
    McpCaller,
    /// Session inventory, a consumer of session events.
    Inventory,
    /// Message dispatch, workers and native control: users of N10.
    CodexClientUser,
    /// The provider supervisor's `ProviderConversationBackend`. N11's conversation operations and
    /// the automation paths call it directly today, bypassing N4: a current structural defect D3
    /// removes (Program Design #N4).
    ProviderSupervisor,
}

/// One row of the Program Design's producer/consumer table.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageFamily {
    CodexClientRequestFromCli,
    CodexMessageToCli,
    CodexRequestToCodex,
    CodexMessageFromCodex,
    SessionCommand,
    PromptContents,
    AcpToAgent,
    AcpSessionUpdate,
    SessionEvent,
    ApprovalRequest,
    ApprovalDecision,
    AcpRequestToCodexAdapter,
    AcpServedForCodexSessions,
    AcpServedForRouterSessions,
    McpToolCall,
    McpToolSchema,
    ClaudeModelMetadata,
}

/// Whether the node took the message in or sent it out.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TapDirection {
    Consumed,
    Produced,
}

/// One message observed at a node's edge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapRecord {
    pub direction: TapDirection,
    pub family: MessageFamily,
    pub counterpart: EdgeParty,
    /// A reply travels on its request's edge, in the other direction, under the request's row
    /// (Program Design #producers-consumers, "Replies").
    pub reply: bool,
    /// The message's own kind, such as `ItemStarted(AgentMessage)` or
    /// `turn/started`. Compared exactly with the catalog row.
    pub kind: String,
    /// What the record holds: a wire frame, a known body, or a typed value that never crossed a
    /// wire.
    #[serde(flatten)]
    pub layer: RecordLayer,
    pub connection: ConnectionId,
    #[serde(flatten)]
    pub intent: RecordIntent,
    /// The decoded content the case asserts on (volatile fields made stable with [`stable`]).
    /// Compared exactly with the catalog row's declared payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    /// The message exactly as it crossed, kept unchanged (E30). Wire-frame and body records must
    /// carry it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    /// The method a reply answers, for a case that starts midway through an exchange.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers: Option<String>,
    /// The edge checker's result, filled in by [`TapCase::finish`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conformance: Option<EdgeConformance>,
    /// Fixture setup on a complete capture (initialize, session setup): checked and counted like
    /// every record, but outside the case's own view, so the catalog row does not compare it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub setup: bool,
}

/// One message a catalog row declares: its family, counterpart, kind and optional payload.
type DeclaredMessage = (MessageFamily, EdgeParty, String, Option<serde_json::Value>);

/// Replaces each field named in `volatile` (at any depth) with `"<volatile:NAME>"`, so a
/// per-run generated id or timestamp does not decide a comparison. Only generated values belong
/// here; content a catalog row talks about stays literal.
#[must_use]
pub fn stable(mut value: serde_json::Value, volatile: &[&str]) -> serde_json::Value {
    fn walk(value: &mut serde_json::Value, volatile: &[&str]) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    if volatile.contains(&key.as_str()) {
                        *field = serde_json::Value::String(format!("<volatile:{key}>"));
                    } else {
                        walk(field, volatile);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                items.iter_mut().for_each(|item| walk(item, volatile))
            }
            _ => {}
        }
    }
    walk(&mut value, volatile);
    value
}

/// The declared state a node owns, as named fields with rendered values.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct NodeStateSnapshot(BTreeMap<String, String>);

impl NodeStateSnapshot {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn field(mut self, name: &str, value: impl fmt::Display) -> Self {
        self.0.insert(name.to_owned(), value.to_string());
        self
    }
}

/// What the catalog row says one input must produce and leave behind.
#[derive(Clone, Debug, Default)]
pub struct CatalogRowExpectation {
    produced: Vec<DeclaredMessage>,
    state_after: Option<NodeStateSnapshot>,
}

impl CatalogRowExpectation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the next produced message, in order.
    #[must_use]
    pub fn produces(mut self, family: MessageFamily, counterpart: EdgeParty, kind: &str) -> Self {
        self.produced
            .push((family, counterpart, kind.to_owned(), None));
        self
    }

    /// Declares the next produced message with its exact decoded payload, in order.
    #[must_use]
    pub fn produces_with(
        mut self,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        payload: serde_json::Value,
    ) -> Self {
        self.produced
            .push((family, counterpart, kind.to_owned(), Some(payload)));
        self
    }

    /// Declares the state the node must hold after the input.
    #[must_use]
    pub fn state_after(mut self, state: NodeStateSnapshot) -> Self {
        self.state_after = Some(state);
        self
    }
}

/// The comparison of one recorded case with its catalog row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "verdict", content = "differences", rename_all = "camelCase")]
pub enum TapVerdict {
    MatchesCatalog,
    DiffersFromCatalog(Vec<String>),
}

/// How the node's behavior stands against its row and its listed behavior defect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum TapOutcome {
    Passed,
    /// Listed, differs from the catalog, and reproduces the declared current
    /// defect exactly.
    ExpectedFailure {
        deliverable: Deliverable,
        defect: String,
    },
    UnexpectedFailure,
    /// Listed, but the recording no longer equals the declared current-defect
    /// behavior (or none was declared): something other than the named defect
    /// is failing.
    DefectChanged {
        deliverable: Deliverable,
        defect: String,
        differences: Vec<String>,
    },
    /// A current-defect behavior was declared for a case the list does not name.
    DefectNotListed,
    UnexpectedPass {
        deliverable: Deliverable,
        defect: String,
    },
}

/// One tap case: a single catalog input driven through one node.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapCase {
    node: ProtocolNode,
    case: String,
    catalog_row: String,
    state_before: NodeStateSnapshot,
    records: Vec<TapRecord>,
    state_after: NodeStateSnapshot,
    /// What today's code does for a listed case: the named defect, exactly.
    #[serde(skip)]
    current_defect: Option<CatalogRowExpectation>,
    /// The connection the next records travel on.
    #[serde(skip)]
    connection: ConnectionId,
    /// The Router-owned codecs this case's records are held to.
    #[serde(skip)]
    codecs: OwnedCodecs,
    /// The ACP agent declared for each connection.
    #[serde(skip)]
    acp_peers: Vec<(ConnectionId, AcpPeer)>,
    /// Record kinds whose missing raw value the case declares, with the reason.
    declared_gaps: Vec<(String, String)>,
}

/// The finished, compared case, as written to the recording directory.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapCaseResult {
    pub case_id: String,
    #[serde(flatten)]
    pub case: TapCase,
    pub comparison: TapVerdict,
    /// The node's behavior against its row and its listed behavior defect.
    pub outcome: TapOutcome,
    /// The recorded messages against their definitions and the listed schema defects.
    pub schema: SchemaSettlement,
}

impl TapCase {
    /// Starts a case. `case` is unique within the node; `catalog_row` names
    /// the Program Design row it exercises.
    #[must_use]
    pub fn new(node: ProtocolNode, case: &str, catalog_row: &str) -> Self {
        Self {
            node,
            case: case.to_owned(),
            catalog_row: catalog_row.to_owned(),
            state_before: NodeStateSnapshot::new(),
            records: Vec::new(),
            state_after: NodeStateSnapshot::new(),
            current_defect: None,
            connection: ConnectionId::case_default(),
            codecs: OwnedCodecs::default(),
            acp_peers: Vec::new(),
            declared_gaps: Vec::new(),
        }
    }

    /// Declares today's defective behavior for a case on the expected-failure
    /// list. The case settles as an expected failure only while the recording
    /// equals this behavior, so an unrelated regression cannot hide behind the
    /// listed defect.
    pub fn current_defect(&mut self, behavior: CatalogRowExpectation) {
        self.current_defect = Some(behavior);
    }

    /// The identity used by the expected-failure list: `<node>/<case>`.
    #[must_use]
    pub fn case_id(&self) -> String {
        format!("{}/{}", self.node.label(), self.case)
    }

    pub fn state_before(&mut self, state: NodeStateSnapshot) {
        self.state_before = state;
    }

    pub fn state_after(&mut self, state: NodeStateSnapshot) {
        self.state_after = state;
    }

    /// The records so far, in order.
    #[must_use]
    pub fn records(&self) -> &[TapRecord] {
        &self.records
    }

    /// Compares the recording with its catalog row without consulting the
    /// expected-failure list.
    #[must_use]
    pub fn compare(&self, expectation: &CatalogRowExpectation) -> TapVerdict {
        let differences = self.differences_from(expectation, "catalog");
        if differences.is_empty() {
            TapVerdict::MatchesCatalog
        } else {
            TapVerdict::DiffersFromCatalog(differences)
        }
    }

    fn differences_from(
        &self,
        expectation: &CatalogRowExpectation,
        reference: &str,
    ) -> Vec<String> {
        let produced: Vec<DeclaredMessage> = self
            .records
            .iter()
            .filter(|record| record.direction == TapDirection::Produced && !record.setup)
            .map(|record| {
                (
                    record.family,
                    record.counterpart,
                    record.kind.clone(),
                    record.payload.clone(),
                )
            })
            .collect();
        let mut differences = Vec::new();
        if produced != expectation.produced {
            differences.push(format!(
                "produced {produced:?}, {reference} declares {:?}",
                expectation.produced
            ));
        }
        if let Some(expected_state) = &expectation.state_after
            && expected_state != &self.state_after
        {
            differences.push(format!(
                "state after {:?}, {reference} declares {:?}",
                self.state_after.0, expected_state.0
            ));
        }
        differences
    }

    /// Compares the case with its row, applies the expected-failure list,
    /// writes the recording when a recording directory is set, and returns the
    /// result. Use [`TapCaseResult::assert_settled`] to fail the test.
    #[must_use]
    pub fn finish(self, expectation: &CatalogRowExpectation) -> TapCaseResult {
        let all: Vec<ExpectedFailure> = EXPECTED_FAILURES
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect();
        let result = self.finish_against(expectation, &all);
        result.write_recording();
        result
    }

    /// [`Self::finish`] against a given expected-failure list, without writing a recording (the
    /// recorder's own tests use it so they never reach the report).
    #[must_use]
    pub fn finish_against(
        mut self,
        expectation: &CatalogRowExpectation,
        expected_failures: &[ExpectedFailure],
    ) -> TapCaseResult {
        let case_id = self.case_id();
        let entry = expected_failures
            .iter()
            .find(|entry| entry.case_id == case_id);
        let schema = self.check_records(entry.map_or(&[][..], |entry| entry.schema));
        let comparison = self.compare(expectation);
        let behavior = entry.and_then(|entry| entry.behavior);
        let outcome = match (&comparison, behavior) {
            (_, None) if self.current_defect.is_some() => TapOutcome::DefectNotListed,
            (TapVerdict::MatchesCatalog, None) => TapOutcome::Passed,
            (TapVerdict::DiffersFromCatalog(_), None) => TapOutcome::UnexpectedFailure,
            (TapVerdict::MatchesCatalog, Some(listed)) => TapOutcome::UnexpectedPass {
                deliverable: listed.deliverable,
                defect: listed.defect.to_owned(),
            },
            (TapVerdict::DiffersFromCatalog(_), Some(listed)) => {
                let differences = match &self.current_defect {
                    Some(defect) => self.differences_from(defect, "current defect"),
                    None => vec!["no current-defect behavior declared".to_owned()],
                };
                if differences.is_empty() {
                    TapOutcome::ExpectedFailure {
                        deliverable: listed.deliverable,
                        defect: listed.defect.to_owned(),
                    }
                } else {
                    TapOutcome::DefectChanged {
                        deliverable: listed.deliverable,
                        defect: listed.defect.to_owned(),
                        differences,
                    }
                }
            }
        };
        TapCaseResult {
            case_id,
            case: self,
            comparison,
            outcome,
            schema,
        }
    }
}

impl TapCaseResult {
    /// Panics unless the case's behavior passed or failed exactly as its listed defect, and its
    /// recorded messages showed exactly its listed schema defects with no coverage gap.
    #[expect(
        clippy::panic,
        reason = "A tap case is a test assertion: an unsettled case must fail the calling test."
    )]
    pub fn assert_settled(&self) {
        if !self.schema.is_settled() {
            let schema = &self.schema;
            panic!(
                "{}: its recorded messages do not settle: unlisted violations {:#?}; listed \
                 defects no longer shown (remove them) {:#?}; coverage gaps {:#?}; injected \
                 violations not shown {:#?}; behavior outcome {:?}",
                self.case_id,
                schema.unlisted,
                schema.fixed,
                schema.coverage_gaps,
                schema.injections_not_shown,
                self.outcome
            );
        }
        match &self.outcome {
            TapOutcome::Passed | TapOutcome::ExpectedFailure { .. } => {}
            TapOutcome::UnexpectedFailure => panic!(
                "{} differs from its catalog row {}: {:?}",
                self.case_id, self.case.catalog_row, self.comparison
            ),
            TapOutcome::DefectChanged {
                deliverable,
                defect,
                differences,
            } => panic!(
                "{} no longer reproduces its listed defect \"{defect}\" ({deliverable:?}): \
                 {differences:?}",
                self.case_id
            ),
            TapOutcome::DefectNotListed => panic!(
                "{} declares a current defect but has no expected-failure entry",
                self.case_id
            ),
            TapOutcome::UnexpectedPass {
                deliverable,
                defect,
            } => panic!(
                "{} now matches its catalog row; {deliverable:?} fixed \"{defect}\", so remove its \
                 expected-failure entry",
                self.case_id
            ),
        }
    }

    #[expect(
        clippy::panic,
        reason = "A recording that cannot be written must fail the tap test that asked for it."
    )]
    fn write_recording(&self) {
        let Some(directory) = std::env::var_os(NODE_TAP_DIR_ENV).map(PathBuf::from) else {
            return;
        };
        let node_directory = directory.join(self.case.node.label());
        let path = node_directory.join(format!("{}.json", self.case.case));
        let body = serde_json::to_vec_pretty(self)
            .unwrap_or_else(|error| panic!("encode tap recording {}: {error}", self.case_id));
        std::fs::create_dir_all(&node_directory)
            .and_then(|()| std::fs::write(&path, body))
            .unwrap_or_else(|error| panic!("write tap recording {}: {error}", path.display()));
    }
}

#[cfg(test)]
#[path = "node_taps_test.rs"]
mod tests;
