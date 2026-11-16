//! The edge checker (D0 R52, Program Design Y2): picks the definition a recorded message is held
//! to, pairs replies with their requests, and checks the message as it crossed.
//!
//! The definition follows from the message's owner (Program Design "Who owns each contract"):
//! Codex messages against the pinned Codex export and pairing; standard ACP methods against the
//! concrete type the pinned ACP schema declares for that method and side; Claude's upstream
//! methods, notifications and declared `_meta` against the Claude extras pin; Router-owned
//! extensions and `_meta` keys against the Rust code that owns them today; MCP envelopes against the `rmcp` types
//! and tool arguments and results against the advertised schemas. Cursor's extras have no
//! authoritative definition. Router vocabulary between parts is never checked as JSON, and
//! typed records are counted but never checked.
//!
//! The pinned definitions live in `verification/schemas/` with their SHA-256 in `pins.json`;
//! [`EdgeDefinitions::load`] refuses to start when a file is missing or its hash differs.

mod acp_definition;
mod codex_definition;
mod mcp_definition;
mod owned_meta;
mod pins;
mod router_extension;

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use serde::Serialize;
use serde_json::{Value, json};

use super::connection_ledger::{ConnectionId, ConnectionLedger};
use super::{MessageFamily, ProtocolNode, TapDirection};
use acp_definition::AcpDefinition;
use codex_definition::CodexDefinition;
pub use pins::{PinError, pinned_schema_directory};
pub use router_extension::{OwnedCodec, OwnedCodecs, OwnedVerdicts, RouterContract};

/// An edge between the Router and a party outside it. Each fixes both protocol roles.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutsideEdge {
    /// Codex CLI (client) to the Router's N1 (app-server).
    CliCodex,
    /// The Router's N10 or N8 (client) to managed Codex (app-server).
    ManagedCodex,
    /// The Router's N6 (ACP client) to Claude or Cursor (ACP agent).
    AcpAgent,
    /// ACP clients to the Router's N8 Codex adapter (ACP agent).
    AcpServedN8,
    /// ACP clients to the Router's N9 provider route (ACP agent).
    AcpServedN9,
    /// MCP callers (client) to the Router's N11 (server).
    Mcp,
}

/// The role a message's sender plays in the edge's protocol, not the part's direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolRole {
    /// Codex and MCP: the client. ACP: the client.
    Client,
    /// Codex: the app-server. MCP: the server. ACP: the agent.
    Server,
}

impl ProtocolRole {
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }
}

/// A known body at a named boundary, checked without its envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "root", rename_all = "camelCase")]
pub enum BodyRoot {
    /// The params of a standard ACP request the sender sends.
    AcpRequestParams { method: String },
    /// The value of a Router-owned `_meta` key, such as `codexRouter.modelReasoning`.
    RouterMeta { contract: RouterContract },
}

/// What a record holds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "layer", rename_all = "camelCase")]
pub enum RecordLayer {
    /// A whole JSON-RPC frame as it crossed a transport.
    WireFrame,
    /// A known body at a named boundary.
    Body { body: BodyRoot },
    /// A decoded value handed to a part in a unit test.
    TypedInput,
    /// A Rust API result: never a wire message.
    TypedOutcome,
}

/// Who owns the contract a message is held to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContractOwner {
    Codex,
    Acp,
    ClaudeExtra,
    RouterExtension,
    CursorExtra,
    Mcp,
    RouterVocabulary,
}

/// What the recording lacks for the message to be judged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "missing", rename_all = "camelCase")]
pub enum MissingContext {
    /// A reply whose request this recording did not hold.
    OrphanReply { id: Value },
    /// A method the owner's definition does not pair with a response.
    NoPairing { method: String },
    /// An extension method no owner in the table claims.
    UnknownMethod { method: String },
    /// An extension method on an ACP connection whose agent (Claude or Cursor) is not known, so
    /// its owner cannot be chosen.
    UnknownPeer { method: String },
    /// A support decision that needs the connection's `initialize`, which was not captured.
    NoNegotiation { capability: String },
    /// A Router-owned contract with no codec registered for the case.
    NoOwnerCodec { contract: RouterContract },
    /// A tool call whose tool the connection never advertised.
    NoAdvertisement { tool: String },
    /// A Router-owned `_meta` key (`router`, `sessionProfile`, `codexRouter`, `codex-router/*`)
    /// no owner check covers: never a silent pass. An open owner at an ancestor does not cover it.
    MissingOwner { pointer: String },
    /// An owner check that could not give its outcome: its environment failed (node unavailable,
    /// a nonzero exit, unreadable output), or the owner's own outcome cannot say whether it
    /// accepted the value. Named, never turned into a pass or a fabricated rejection.
    OwnerOutcomeUnavailable {
        contract: RouterContract,
        reason: String,
    },
    /// The value is not a JSON-RPC frame.
    NotJsonRpc,
}

/// One rule a message breaks.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub schema_path: String,
    pub instance_path: String,
    /// The rule: the JSON Schema keyword, or `method`, `routerCodec`, `rmcpDecode`, `codexParts`.
    pub keyword: String,
    /// The rule's own discriminator: the missing property for `required`, the unexpected names
    /// for `additionalProperties`, the method for `method`; empty when the keyword has none.
    pub detail: String,
    pub expected: String,
    pub found: String,
}

/// Whether the receiving peer negotiated what a well-shaped message uses.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "support", rename_all = "camelCase")]
pub enum Support {
    Negotiated,
    NotNegotiated { capability: String },
    NotApplicable,
}

/// The result of checking one record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum EdgeConformance {
    Conforms {
        owner: ContractOwner,
        definition: String,
        support: Support,
    },
    /// Every violation, not the first.
    Violations {
        owner: ContractOwner,
        definition: String,
        violations: Vec<Violation>,
    },
    Incomplete {
        owner: ContractOwner,
        missing: MissingContext,
    },
    NoAuthoritativeDefinition {
        owner: ContractOwner,
        method: String,
    },
    /// A typed record, or Router vocabulary between parts: counted and listed, never checked.
    NotSchemaChecked { layer: RecordLayer },
    /// A frame held to more than one owner (the standard ACP definition and the owners of its
    /// `_meta` keys): every component's verdict, none dropped and none hiding another.
    Composite { parts: Vec<EdgeConformance> },
}

impl EdgeConformance {
    /// The frame's verdicts merged: one part is itself; more parts are a `Composite`, so a listed
    /// violation of one owner never hides another owner's violation or missing check.
    #[must_use]
    pub fn merged(mut parts: Vec<Self>) -> Self {
        if parts.len() == 1 {
            return parts.remove(0);
        }
        Self::Composite { parts }
    }

    /// Every leaf verdict, a `Composite`'s parts in order.
    pub fn leaves(&self) -> Box<dyn Iterator<Item = &Self> + '_> {
        match self {
            Self::Composite { parts } => Box::new(parts.iter().flat_map(Self::leaves)),
            leaf => Box::new(std::iter::once(leaf)),
        }
    }
}

/// One record to check, with the context that chooses its definition.
pub struct CheckInput<'a> {
    pub edge: OutsideEdge,
    pub connection: &'a ConnectionId,
    pub sender: ProtocolRole,
    pub layer: &'a RecordLayer,
    /// The method a reply answers, for a case that starts midway through an exchange.
    pub answers: Option<&'a str>,
    pub raw: &'a Value,
    pub codecs: &'a OwnedCodecs,
}

/// The edge a family crosses and whether `recorder`, the part that recorded the message, plays
/// that edge's server.
#[must_use]
pub fn family_edge(family: MessageFamily, recorder: ProtocolNode) -> Option<(OutsideEdge, bool)> {
    match family {
        MessageFamily::CodexClientRequestFromCli | MessageFamily::CodexMessageToCli => {
            Some((OutsideEdge::CliCodex, true))
        }
        MessageFamily::CodexRequestToCodex | MessageFamily::CodexMessageFromCodex => {
            Some((OutsideEdge::ManagedCodex, false))
        }
        MessageFamily::AcpToAgent
        | MessageFamily::AcpSessionUpdate
        | MessageFamily::ClaudeModelMetadata => Some((OutsideEdge::AcpAgent, false)),
        // N8 serves this edge; any other part recording it (N11's carrier) is its client.
        MessageFamily::AcpRequestToCodexAdapter | MessageFamily::AcpServedForCodexSessions => {
            Some((
                OutsideEdge::AcpServedN8,
                recorder == ProtocolNode::N8CodexAcpAdapter,
            ))
        }
        MessageFamily::AcpServedForRouterSessions => Some((OutsideEdge::AcpServedN9, true)),
        MessageFamily::McpToolCall | MessageFamily::McpToolSchema => Some((OutsideEdge::Mcp, true)),
        MessageFamily::SessionCommand
        | MessageFamily::PromptContents
        | MessageFamily::SessionEvent
        | MessageFamily::ApprovalRequest
        | MessageFamily::ApprovalDecision => None,
    }
}

/// The protocol role of the sender of a message the recording part `direction`-ed on `edge`.
#[must_use]
pub const fn sender_role(part_is_server: bool, direction: TapDirection) -> ProtocolRole {
    match (part_is_server, direction) {
        (true, TapDirection::Produced) | (false, TapDirection::Consumed) => ProtocolRole::Server,
        (true, TapDirection::Consumed) | (false, TapDirection::Produced) => ProtocolRole::Client,
    }
}

/// The pinned true definitions, loaded once with their hashes checked.
pub struct EdgeDefinitions {
    codex: CodexDefinition,
    acp: AcpDefinition,
    validators: Mutex<HashMap<String, Arc<jsonschema::Validator>>>,
}

impl EdgeDefinitions {
    /// Loads the pinned definitions from [`pinned_schema_directory`].
    pub fn load() -> Result<Self, PinError> {
        Self::load_from(&pinned_schema_directory())
    }

    /// Loads the pinned definitions from `directory`.
    pub fn load_from(directory: &Path) -> Result<Self, PinError> {
        let mut files = pins::PinnedFiles::read(directory)?;
        let codex = CodexDefinition::new(
            files.take(pins::CODEX_BUNDLE)?,
            &files.take(pins::CODEX_PAIRING)?,
        )
        .map_err(|error| PinError::Invalid {
            file: pins::CODEX_PAIRING.to_owned(),
            error,
        })?;
        let acp = AcpDefinition::new(
            files.take(pins::ACP_SCHEMA)?,
            files.take(pins::CLAUDE_EXTRAS)?,
        )
        .map_err(|error| PinError::Invalid {
            file: pins::ACP_SCHEMA.to_owned(),
            error,
        })?;
        Ok(Self {
            codex,
            acp,
            validators: Mutex::new(HashMap::new()),
        })
    }

    /// The process-wide definitions every tap case checks against.
    ///
    /// # Panics
    /// When a pinned file is missing or its hash changed: every part test then fails at its start,
    /// naming the file (Program Design, "When something goes wrong").
    #[expect(
        clippy::panic,
        reason = "A tap test cannot check its edges without the pinned definitions; it must fail."
    )]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Result<EdgeDefinitions, PinError>> = OnceLock::new();
        match SHARED.get_or_init(Self::load) {
            Ok(definitions) => definitions,
            Err(error) => panic!("{error}"),
        }
    }

    /// Checks one record, updating `ledger` with the requests, negotiations and advertisements
    /// it carries.
    pub fn check(&self, input: &CheckInput<'_>, ledger: &mut ConnectionLedger) -> EdgeConformance {
        match input.layer {
            RecordLayer::TypedInput | RecordLayer::TypedOutcome => {
                EdgeConformance::NotSchemaChecked {
                    layer: input.layer.clone(),
                }
            }
            RecordLayer::Body {
                body: BodyRoot::RouterMeta { contract },
            } => router_extension::check(input.codecs, *contract, input.raw),
            RecordLayer::Body {
                body: BodyRoot::AcpRequestParams { method },
            } => self.acp.check_params(self, input.sender, method, input.raw),
            RecordLayer::WireFrame => match input.edge {
                OutsideEdge::CliCodex | OutsideEdge::ManagedCodex => {
                    self.codex.check(self, input, ledger)
                }
                OutsideEdge::AcpAgent | OutsideEdge::AcpServedN8 | OutsideEdge::AcpServedN9 => {
                    self.acp.check(self, input, ledger)
                }
                OutsideEdge::Mcp => mcp_definition::check(self, input, ledger),
            },
        }
    }

    /// The pinned Codex export's `definitions.v2`, the reference for MCP's Codex parts.
    pub(crate) fn codex_v2_definitions(&self) -> &Value {
        self.codex.v2_definitions()
    }

    /// Validates `instance` against the schema `build_schema` makes, compiling and caching one
    /// validator per `cache_key`. An empty list conforms.
    pub(crate) fn validate(
        &self,
        cache_key: &str,
        build_schema: impl FnOnce() -> Value,
        instance: &Value,
    ) -> Result<Vec<Violation>, String> {
        let validator = {
            let mut validators = self
                .validators
                .lock()
                .map_err(|_| "the validator cache lock is poisoned".to_owned())?;
            if let Some(validator) = validators.get(cache_key) {
                Arc::clone(validator)
            } else {
                let compiled = Arc::new(
                    jsonschema::options()
                        .should_validate_formats(false)
                        .build(&build_schema())
                        .map_err(|error| format!("compile {cache_key}: {error}"))?,
                );
                validators.insert(cache_key.to_owned(), Arc::clone(&compiled));
                compiled
            }
        };
        Ok(violations_of(validator.iter_errors(instance)))
    }
}

/// Collects every validation error as a violation, sorted and without repeats.
pub(crate) fn violations_of<'a>(
    errors: impl Iterator<Item = jsonschema::ValidationError<'a>>,
) -> Vec<Violation> {
    let mut violations: Vec<Violation> = errors
        .map(|error| {
            let schema_path = error.schema_path().to_string();
            let keyword = schema_path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            Violation {
                instance_path: error.instance_path().to_string(),
                keyword,
                detail: keyword_detail(error.kind()),
                expected: error.to_string(),
                found: compact(error.instance()),
                schema_path,
            }
        })
        .collect();
    violations.sort();
    violations.dedup();
    violations
}

/// The discriminator jsonschema 0.53's error kind carries for its keyword.
fn keyword_detail(kind: &jsonschema::error::ValidationErrorKind) -> String {
    use jsonschema::error::ValidationErrorKind;
    match kind {
        ValidationErrorKind::Required { property } => property
            .as_str()
            .map_or_else(|| property.to_string(), str::to_owned),
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected } => unexpected.join(","),
        _ => String::new(),
    }
}

/// Renders a found value briefly for a violation report.
pub(crate) fn compact(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 160 {
        format!("{}…", text.chars().take(160).collect::<String>())
    } else {
        text
    }
}

/// Opens `id` in the ledger and merges a duplicate-outstanding-id violation into `verdict` when
/// the id is already waiting on this connection (JSON-RPC 2.0: an id identifies one open request).
pub(crate) fn with_request_opened(
    ledger: &mut ConnectionLedger,
    input: &CheckInput<'_>,
    owner: ContractOwner,
    id: &Value,
    method: &str,
    verdict: EdgeConformance,
) -> EdgeConformance {
    match ledger.open_request(input.connection, input.sender, id, method) {
        Ok(()) => verdict,
        Err(duplicate) => EdgeConformance::merged(vec![
            verdict,
            EdgeConformance::Violations {
                owner,
                definition: "JSON-RPC 2.0 request id".to_owned(),
                violations: vec![Violation {
                    schema_path: "jsonrpc/id".to_owned(),
                    instance_path: "/id".to_owned(),
                    keyword: "duplicateId".to_owned(),
                    detail: method.to_owned(),
                    expected: format!(
                        "an id no request on this connection is still waiting on; {} is waiting on it",
                        duplicate.first_method
                    ),
                    found: compact(&duplicate.id),
                }],
            },
        ]),
    }
}

/// A conformance from a list of violations against `definition`.
pub(crate) fn judged(
    owner: ContractOwner,
    definition: String,
    violations: Result<Vec<Violation>, String>,
    support: Support,
) -> EdgeConformance {
    match violations {
        Ok(violations) if violations.is_empty() => EdgeConformance::Conforms {
            owner,
            definition,
            support,
        },
        Ok(violations) => EdgeConformance::Violations {
            owner,
            definition,
            violations,
        },
        Err(problem) => EdgeConformance::Violations {
            owner,
            definition,
            violations: vec![Violation {
                schema_path: String::new(),
                instance_path: String::new(),
                keyword: "compile".to_owned(),
                detail: String::new(),
                expected: "a definition the checker can compile".to_owned(),
                found: problem,
            }],
        },
    }
}

/// A violation of the method table: the method is not one the owner declares for this sender.
pub(crate) fn unknown_method_violation(table: &str, method: &str, what: &str) -> Violation {
    Violation {
        schema_path: table.to_owned(),
        instance_path: "/method".to_owned(),
        keyword: "method".to_owned(),
        detail: method.to_owned(),
        expected: format!("a {what} method the definition declares for this sender"),
        found: json!(method).to_string(),
    }
}

/// The JSON-RPC shape of one frame.
pub(crate) enum RpcShape<'a> {
    Request {
        id: &'a Value,
        method: &'a str,
    },
    Notification {
        method: &'a str,
    },
    Success {
        id: &'a Value,
        result: &'a Value,
    },
    Error {
        id: &'a Value,
    },
    /// A reply with neither `result` nor `error` (or both): it answers its request but carries no
    /// valid body, so it pairs and is judged as a required-body violation (correction 4, V3).
    BareReply {
        id: &'a Value,
    },
}

pub(crate) fn rpc_shape(wire: &Value) -> Option<RpcShape<'_>> {
    let object = wire.as_object()?;
    match (
        object.get("id"),
        object.get("method").and_then(Value::as_str),
    ) {
        (Some(id), Some(method)) => Some(RpcShape::Request { id, method }),
        (None, Some(method)) => Some(RpcShape::Notification { method }),
        (Some(id), None) => match (object.get("result"), object.get("error")) {
            (Some(result), None) => Some(RpcShape::Success { id, result }),
            (None, Some(_)) => Some(RpcShape::Error { id }),
            _ => Some(RpcShape::BareReply { id }),
        },
        (None, None) => None,
    }
}

/// A reply carrying neither `result` nor `error`, for a method with no response definition.
pub(crate) fn bare_reply_violation(owner: ContractOwner, method: &str) -> EdgeConformance {
    EdgeConformance::Violations {
        owner,
        definition: format!("JSON-RPC 2.0 response:{method}"),
        violations: vec![Violation {
            schema_path: "JSON-RPC 2.0 response".to_owned(),
            instance_path: String::new(),
            keyword: "required".to_owned(),
            detail: "result".to_owned(),
            expected: "a reply carrying result or error".to_owned(),
            found: "neither".to_owned(),
        }],
    }
}

/// A reply carrying both `result` and `error`: JSON-RPC 2.0 gives a reply exactly one of them
/// (correction 5, C4-2).
pub(crate) fn both_bodies_violation(owner: ContractOwner, method: &str) -> EdgeConformance {
    EdgeConformance::Violations {
        owner,
        definition: format!("JSON-RPC 2.0 response:{method}"),
        violations: vec![Violation {
            schema_path: "JSON-RPC 2.0 response".to_owned(),
            instance_path: String::new(),
            keyword: "oneOf".to_owned(),
            detail: "result,error".to_owned(),
            expected: "a reply carrying exactly one of result and error".to_owned(),
            found: "both".to_owned(),
        }],
    }
}

/// The body violation of a reply that is neither a plain result nor a plain error.
pub(crate) fn bodiless_or_double_reply(
    raw: &Value,
    owner: ContractOwner,
    method: &str,
) -> EdgeConformance {
    if raw.get("result").is_some() && raw.get("error").is_some() {
        both_bodies_violation(owner, method)
    } else {
        bare_reply_violation(owner, method)
    }
}

/// The method a reply answers: its ledger entry, or the record's explicit `answers`.
pub(crate) fn answered_method(
    input: &CheckInput<'_>,
    ledger: &mut ConnectionLedger,
    id: &Value,
) -> Option<String> {
    ledger
        .close_reply(input.connection, input.sender, id)
        .or_else(|| input.answers.map(str::to_owned))
}

#[cfg(test)]
#[path = "edge_conformance_test.rs"]
mod tests;
