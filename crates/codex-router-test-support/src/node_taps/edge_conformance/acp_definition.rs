//! ACP frames against the pinned schema of the ACP library the Router compiles
//! (`agent-client-protocol-schema` 1.9.1, default features), plus the extensions' owners.
//!
//! The pinned schema's root unions are untagged (`anyOf` over params types, with `ExtRequest`
//! accepting anything), so they cannot say which params belong to which method. Every request,
//! response and notification definition carries `x-method` and `x-side` (the side that handles
//! it); the checker loads those as explicit roots and checks each frame's envelope and its params
//! or result against the one definition its method names, never the extension fallback.
//!
//! A method whose contract requires a body (its params definition rejects an empty object) must
//! carry `params`; a result always carries `result`.
//!
//! Extensions, by owner and edge: Claude's upstream methods (`_session/steering`, `_session/goal`)
//! and its agent notification `_auth/status_update` against the Claude extras pin; the Router's own
//! `_session/queue/*`, `_session/state` and `_session/steering`, served by N9, against their
//! owning codecs; Cursor's `cursor/*` has no authoritative definition. Any other extension method
//! is an unknown method, never a silent pass.
//!
//! `_meta` is open in ACP, so every frame's owned `_meta` places are also held to their owners
//! (`owned_meta`), and the frame's verdict merges all of them: a violation of one owner never
//! hides another's, and a Router-owned key with no owner check is reported, not passed.

use super::super::json_path::at;
use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::super::connection_ledger::{AcpPeer, ConnectionLedger};
use super::owned_meta::{self, ClaudeMetaPins, FramePlace, MetaOwner};
use super::router_extension::{self, RouterContract};
use super::{
    CheckInput, ContractOwner, EdgeConformance, EdgeDefinitions, MissingContext, OutsideEdge,
    ProtocolRole, RpcShape, Support, Violation, answered_method, bodiless_or_double_reply,
    both_bodies_violation, judged, rpc_shape, unknown_method_violation, with_request_opened,
};

pub(super) struct AcpDefinition {
    /// The ACP pin's `$defs` merged with the Claude extras pin's `$defs`.
    defs: Map<String, Value>,
    /// `(sender, method)` to the params definition name.
    requests: HashMap<(Side, String), String>,
    notifications: HashMap<(Side, String), String>,
    /// `(request sender, method)` to the result definition name.
    responses: HashMap<(Side, String), String>,
    /// Claude's upstream methods: params and result definition names.
    claude_methods: HashMap<String, (String, String)>,
    /// Claude's upstream notifications the agent sends: params definition name.
    claude_notifications: HashMap<String, String>,
    /// The Claude pin's `x-meta`: each place's definition and the keys that definition declares.
    claude_meta: ClaudeMetaPins,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Side {
    Client,
    Agent,
}

impl From<ProtocolRole> for Side {
    fn from(role: ProtocolRole) -> Self {
        match role {
            ProtocolRole::Client => Self::Client,
            ProtocolRole::Server => Self::Agent,
        }
    }
}

/// The owner of an extension method on an edge, and how it is judged.
enum ExtensionOwner {
    Claude {
        params: String,
        result: String,
    },
    ClaudeNotification {
        params: String,
    },
    Router,
    Cursor,
    /// An extension method on a connection whose agent is not known.
    UnknownPeer,
    Unknown,
}

fn def_name(pointer: &Value) -> Result<String, String> {
    pointer
        .as_str()
        .and_then(|pointer| pointer.strip_prefix("#/$defs/"))
        .map(str::to_owned)
        .ok_or_else(|| format!("not a #/$defs pointer: {pointer}"))
}

/// A JSON-RPC 2.0 envelope requiring `required`, with `field` checked against `$defs/<name>`.
fn envelope(required: &[&str], field: Option<(&str, &str)>) -> Value {
    let mut properties = json!({
        "jsonrpc": {"const": "2.0"},
        "id": {"$ref": "#/$defs/RequestId"},
        "method": {"type": "string"},
    });
    if let Some((field, name)) = field
        && let Some(fields) = properties.as_object_mut()
    {
        fields.insert(field.to_owned(), json!({"$ref": format!("#/$defs/{name}")}));
    }
    json!({"type": "object", "required": required, "properties": properties})
}

impl AcpDefinition {
    pub(super) fn new(schema: Value, claude: Value) -> Result<Self, String> {
        let mut defs = at(&schema, &["$defs"])
            .as_object()
            .cloned()
            .ok_or("the ACP schema has no $defs")?;
        let mut requests = HashMap::new();
        let mut notifications = HashMap::new();
        let mut responses = HashMap::new();
        for (name, definition) in &defs {
            let (Some(method), Some(handler)) = (
                at(definition, &["x-method"]).as_str(),
                at(definition, &["x-side"]).as_str(),
            ) else {
                continue;
            };
            // `x-side` names the side that handles a request or notification and the side that
            // answers with a response; the sender recorded here is the other side.
            let senders: &[Side] = match handler {
                "agent" => &[Side::Client],
                "client" => &[Side::Agent],
                "protocol" => &[Side::Client, Side::Agent],
                other => return Err(format!("{name} has an unknown x-side {other}")),
            };
            for &sender in senders {
                let key = (sender, method.to_owned());
                let table = if name.ends_with("Request") {
                    &mut requests
                } else if name.ends_with("Notification") {
                    &mut notifications
                } else if name.ends_with("Response") {
                    &mut responses
                } else {
                    return Err(format!(
                        "{name} carries x-method but is no request, response or notification"
                    ));
                };
                table.insert(key, name.clone());
            }
        }
        let mut claude_notifications = HashMap::new();
        for (method, entry) in at(&claude, &["x-notifications"])
            .as_object()
            .into_iter()
            .flatten()
        {
            claude_notifications.insert(method.clone(), def_name(at(entry, &["params"]))?);
        }
        let mut claude_methods = HashMap::new();
        for (method, entry) in at(&claude, &["x-methods"])
            .as_object()
            .into_iter()
            .flatten()
        {
            claude_methods.insert(
                method.clone(),
                (
                    def_name(at(entry, &["params"]))?,
                    def_name(at(entry, &["result"]))?,
                ),
            );
        }
        let mut claude_meta = ClaudeMetaPins::default();
        for (place, pointer) in at(&claude, &["x-meta"]).as_object().into_iter().flatten() {
            claude_meta.places.insert(place.clone(), def_name(pointer)?);
        }
        for (name, definition) in at(&claude, &["$defs"]).as_object().into_iter().flatten() {
            if defs.insert(name.clone(), definition.clone()).is_some() {
                return Err(format!(
                    "the Claude extras define {name}, which ACP defines too"
                ));
            }
        }
        let defs_snapshot = defs.clone();
        Ok(Self {
            defs,
            requests,
            notifications,
            responses,
            claude_methods,
            claude_notifications,
            claude_meta: with_declared_keys(claude_meta, &defs_snapshot),
        })
    }

    /// Whether a method's contract requires a body: its params definition rejects an empty
    /// object, so a frame without `params` cannot carry what the contract needs.
    fn body_required(&self, definitions: &EdgeDefinitions, name: &str) -> bool {
        let schema = json!({"$ref": format!("#/$defs/{name}")});
        self.validate(definitions, &format!("empty:{name}"), &schema, &json!({}))
            .map_or(true, |violations| !violations.is_empty())
    }

    /// The envelope of a request or notification carrying `name` as its params.
    fn params_envelope(&self, definitions: &EdgeDefinitions, name: &str, request: bool) -> Value {
        let mut required = vec!["jsonrpc", "method"];
        if request {
            required.insert(1, "id");
        }
        if self.body_required(definitions, name) {
            required.push("params");
        }
        envelope(&required, Some(("params", name)))
    }

    fn validate(
        &self,
        definitions: &EdgeDefinitions,
        label: &str,
        schema: &Value,
        instance: &Value,
    ) -> Result<Vec<Violation>, String> {
        let build = || {
            let mut document = schema.clone();
            if let Some(fields) = document.as_object_mut() {
                fields.insert(
                    "$schema".to_owned(),
                    json!("https://json-schema.org/draft/2020-12/schema"),
                );
                fields.insert("$defs".to_owned(), Value::Object(self.defs.clone()));
            }
            document
        };
        definitions.validate(&format!("acp:{label}"), build, instance)
    }

    /// Who owns an extension method: by edge plus method, and on the agent edge by the connection's
    /// agent (R52). Claude's pinned methods and notifications belong to Claude; on a Cursor
    /// connection every extension method is Cursor's, which nothing upstream declares.
    fn extension_owner(
        &self,
        edge: OutsideEdge,
        sender: Side,
        method: &str,
        peer: Option<AcpPeer>,
        notification: bool,
    ) -> ExtensionOwner {
        let extension = method.starts_with('_') || method.starts_with("cursor/");
        match edge {
            OutsideEdge::AcpAgent => match peer {
                Some(AcpPeer::Claude) => {
                    if let (Side::Client, false, Some((params, result))) =
                        (sender, notification, self.claude_methods.get(method))
                    {
                        ExtensionOwner::Claude {
                            params: params.clone(),
                            result: result.clone(),
                        }
                    } else if let (Side::Agent, true, Some(params)) =
                        (sender, notification, self.claude_notifications.get(method))
                    {
                        ExtensionOwner::ClaudeNotification {
                            params: params.clone(),
                        }
                    } else {
                        ExtensionOwner::Unknown
                    }
                }
                Some(AcpPeer::Cursor) if extension => ExtensionOwner::Cursor,
                Some(AcpPeer::Cursor) => ExtensionOwner::Unknown,
                None if extension => ExtensionOwner::UnknownPeer,
                None => ExtensionOwner::Unknown,
            },
            OutsideEdge::AcpServedN9 if RouterContract::for_method(method, false).is_some() => {
                ExtensionOwner::Router
            }
            _ => ExtensionOwner::Unknown,
        }
    }

    /// A body recorded at a named boundary: the params of a standard request `sender` sends.
    pub(super) fn check_params(
        &self,
        definitions: &EdgeDefinitions,
        sender: ProtocolRole,
        method: &str,
        params: &Value,
    ) -> EdgeConformance {
        let Some(name) = self.requests.get(&(Side::from(sender), method.to_owned())) else {
            return EdgeConformance::Incomplete {
                owner: ContractOwner::Acp,
                missing: MissingContext::UnknownMethod {
                    method: method.to_owned(),
                },
            };
        };
        let schema = json!({"$ref": format!("#/$defs/{name}")});
        judged(
            ContractOwner::Acp,
            name.clone(),
            self.validate(definitions, &format!("body:{name}"), &schema, params),
            Support::NotApplicable,
        )
    }

    pub(super) fn check(
        &self,
        definitions: &EdgeDefinitions,
        input: &CheckInput<'_>,
        ledger: &mut ConnectionLedger,
    ) -> EdgeConformance {
        let side = Side::from(input.sender);
        let Some(shape) = rpc_shape(input.raw) else {
            // A malformed frame still reports every Router-owned key it carries.
            return self.with_unowned_meta(
                incomplete(ContractOwner::Acp, MissingContext::NotJsonRpc),
                input,
            );
        };
        match shape {
            RpcShape::Request { id, method } => {
                let verdict = if let Some(name) = self.requests.get(&(side, method.to_owned())) {
                    let schema = self.params_envelope(definitions, name, true);
                    let violations = self.validate(definitions, name, &schema, input.raw);
                    let support = prompt_support(method, input, ledger);
                    support_judged(ContractOwner::Acp, name, violations, support)
                } else {
                    self.check_extension(definitions, input, ledger, method, false)
                };
                let verdict = self.with_owned_meta(
                    definitions,
                    verdict,
                    input,
                    ledger,
                    FramePlace::Request,
                    method,
                    false,
                );
                with_request_opened(ledger, input, ContractOwner::Acp, id, method, verdict)
            }
            RpcShape::Notification { method } => {
                let verdict = if let Some(name) = self.notifications.get(&(side, method.to_owned()))
                {
                    let schema = self.params_envelope(definitions, name, false);
                    let violations = self.validate(definitions, name, &schema, input.raw);
                    judged(
                        ContractOwner::Acp,
                        name.clone(),
                        violations,
                        Support::NotApplicable,
                    )
                } else {
                    self.check_extension(definitions, input, ledger, method, false)
                };
                self.with_owned_meta(
                    definitions,
                    verdict,
                    input,
                    ledger,
                    FramePlace::Notification { method },
                    method,
                    false,
                )
            }
            RpcShape::BareReply { id } => {
                let Some(method) = answered_method(input, ledger, id) else {
                    return self.with_unowned_meta(
                        incomplete(
                            ContractOwner::Acp,
                            MissingContext::OrphanReply { id: id.clone() },
                        ),
                        input,
                    );
                };
                // A reply with neither result nor error answers its request but carries no body:
                // judged against that method's response envelope, which requires `result`.
                let request_sender = Side::from(input.sender.other());
                let verdict = match self.responses.get(&(request_sender, method.clone())) {
                    Some(name) => {
                        let schema = envelope(&["jsonrpc", "id", "result"], Some(("result", name)));
                        judged(
                            ContractOwner::Acp,
                            format!("{name}:{method}"),
                            self.validate(definitions, name, &schema, input.raw),
                            Support::NotApplicable,
                        )
                    }
                    None => bodiless_or_double_reply(input.raw, ContractOwner::Acp, &method),
                };
                // Both bodies at once breaks JSON-RPC even when the result fits its envelope.
                let verdict = if self
                    .responses
                    .contains_key(&(request_sender, method.clone()))
                    && input.raw.get("result").is_some()
                    && input.raw.get("error").is_some()
                {
                    EdgeConformance::merged(vec![
                        verdict,
                        both_bodies_violation(ContractOwner::Acp, &method),
                    ])
                } else {
                    verdict
                };
                self.with_unowned_meta(verdict, input)
            }
            RpcShape::Success { id, result } => {
                let Some(method) = answered_method(input, ledger, id) else {
                    // The request method is unknown, so method-specific owners cannot be chosen
                    // and their keys stay missing owners; owners that do not depend on the method
                    // (a Claude result's modelReasoning) still run (correction 5, C4-1).
                    return self.with_owned_meta(
                        definitions,
                        incomplete(
                            ContractOwner::Acp,
                            MissingContext::OrphanReply { id: id.clone() },
                        ),
                        input,
                        ledger,
                        FramePlace::Result { method: "" },
                        "",
                        true,
                    );
                };
                if method == "initialize" {
                    ledger.record_initialize_result(input.connection, result);
                }
                let request_sender = Side::from(input.sender.other());
                let verdict =
                    if let Some(name) = self.responses.get(&(request_sender, method.clone())) {
                        let schema = envelope(&["jsonrpc", "id", "result"], Some(("result", name)));
                        let violations = self.validate(definitions, name, &schema, input.raw);
                        judged(
                            ContractOwner::Acp,
                            format!("{name}:{method}"),
                            violations,
                            Support::NotApplicable,
                        )
                    } else {
                        self.check_extension(definitions, input, ledger, &method, true)
                    };
                self.with_owned_meta(
                    definitions,
                    verdict,
                    input,
                    ledger,
                    FramePlace::Result { method: &method },
                    &method,
                    true,
                )
            }
            RpcShape::Error { id } => {
                if answered_method(input, ledger, id).is_none() {
                    return self.with_unowned_meta(
                        incomplete(
                            ContractOwner::Acp,
                            MissingContext::OrphanReply { id: id.clone() },
                        ),
                        input,
                    );
                }
                let schema = envelope(&["jsonrpc", "id", "error"], Some(("error", "Error")));
                let violations = self.validate(definitions, "Error", &schema, input.raw);
                let verdict = judged(
                    ContractOwner::Acp,
                    "Error".to_owned(),
                    violations,
                    Support::NotApplicable,
                );
                self.with_owned_meta(
                    definitions,
                    verdict,
                    input,
                    ledger,
                    FramePlace::Error,
                    "",
                    true,
                )
            }
        }
    }

    /// An extension request, notification or result (`result` set), judged by its owner.
    fn check_extension(
        &self,
        definitions: &EdgeDefinitions,
        input: &CheckInput<'_>,
        ledger: &ConnectionLedger,
        method: &str,
        result: bool,
    ) -> EdgeConformance {
        // A reply's owner follows its request, which the other side sent.
        let request_side = if result {
            Side::from(input.sender.other())
        } else {
            Side::from(input.sender)
        };
        let notification = !result && at(input.raw, &["id"]).is_null();
        let peer = ledger.peer(input.connection);
        match self.extension_owner(input.edge, request_side, method, peer, notification) {
            ExtensionOwner::ClaudeNotification { params } => {
                let schema = self.params_envelope(definitions, &params, false);
                let violations = self.validate(definitions, &params, &schema, input.raw);
                judged(
                    ContractOwner::ClaudeExtra,
                    params,
                    violations,
                    Support::NotApplicable,
                )
            }
            ExtensionOwner::UnknownPeer => incomplete(
                ContractOwner::Acp,
                MissingContext::UnknownPeer {
                    method: method.to_owned(),
                },
            ),
            ExtensionOwner::Claude {
                params,
                result: result_name,
            } => {
                let (field, name) = if result {
                    ("result", result_name)
                } else {
                    ("params", params)
                };
                let schema = if result {
                    envelope(&["jsonrpc", "id", "result"], Some((field, &name)))
                } else {
                    self.params_envelope(definitions, &name, true)
                };
                let violations = self.validate(definitions, &name, &schema, input.raw);
                let support = if result {
                    Ok(Support::NotApplicable)
                } else {
                    claude_method_support(method, input, ledger)
                };
                support_judged(ContractOwner::ClaudeExtra, &name, violations, support)
            }
            ExtensionOwner::Router => {
                let Some(contract) = RouterContract::for_method(method, result) else {
                    return incomplete(
                        ContractOwner::RouterExtension,
                        MissingContext::NoPairing {
                            method: method.to_owned(),
                        },
                    );
                };
                let body = if result {
                    &at(input.raw, &["result"])
                } else {
                    &at(input.raw, &["params"])
                };
                router_extension::check(input.codecs, contract, body)
            }
            ExtensionOwner::Cursor => EdgeConformance::NoAuthoritativeDefinition {
                owner: ContractOwner::CursorExtra,
                method: method.to_owned(),
            },
            // An extension method no owner in the table claims: never a silent pass.
            ExtensionOwner::Unknown if method.starts_with('_') => incomplete(
                ContractOwner::Acp,
                MissingContext::UnknownMethod {
                    method: method.to_owned(),
                },
            ),
            ExtensionOwner::Unknown => EdgeConformance::Violations {
                owner: ContractOwner::Acp,
                definition: "x-method".to_owned(),
                violations: vec![unknown_method_violation(
                    "/$defs/*/x-method",
                    method,
                    if result {
                        "request answered by a result"
                    } else {
                        "request or notification"
                    },
                )],
            },
        }
    }

    /// Merges the frame's standard verdict with the verdict of every owned `_meta` place
    /// (`owned_meta`), and reports each Router-owned key no owner check covers.
    #[expect(
        clippy::too_many_arguments,
        reason = "The owner table needs the frame's place, method and side; one call site per shape."
    )]
    fn with_owned_meta(
        &self,
        definitions: &EdgeDefinitions,
        verdict: EdgeConformance,
        input: &CheckInput<'_>,
        ledger: &ConnectionLedger,
        place: FramePlace<'_>,
        method: &str,
        result: bool,
    ) -> EdgeConformance {
        let peer = ledger.peer(input.connection);
        let checks = owned_meta::selected(input.edge, place, peer, input.raw, &self.claude_meta);
        // A Router extension body the owning codec already checked whole is not re-reported.
        let body_root = if result { "/result" } else { "/params" };
        let claimed: &[&str] = if input.edge == OutsideEdge::AcpServedN9
            && RouterContract::for_method(method, result).is_some()
        {
            &[body_root]
        } else {
            &[]
        };
        let mut parts = vec![verdict];
        for check in &checks {
            let Some(value) = input.raw.pointer(&check.pointer) else {
                continue;
            };
            let part = match &check.owner {
                MetaOwner::Router(contract) => {
                    router_extension::check(input.codecs, *contract, value)
                }
                MetaOwner::ClaudePin(name) => {
                    let schema = json!({"$ref": format!("#/$defs/{name}")});
                    judged(
                        ContractOwner::ClaudeExtra,
                        name.clone(),
                        self.validate(definitions, &format!("meta:{name}"), &schema, value),
                        Support::NotApplicable,
                    )
                }
            };
            parts.push(at_pointer(part, &check.pointer));
        }
        for pointer in owned_meta::unclaimed(input.raw, &checks, claimed) {
            parts.push(incomplete(
                ContractOwner::RouterExtension,
                MissingContext::MissingOwner { pointer },
            ));
        }
        EdgeConformance::merged(parts)
    }

    /// A frame whose place cannot be known (malformed, orphan, bodiless): no owner is selected,
    /// and every Router-owned key it carries is reported as missing its owner.
    fn with_unowned_meta(
        &self,
        verdict: EdgeConformance,
        input: &CheckInput<'_>,
    ) -> EdgeConformance {
        let mut parts = vec![verdict];
        for pointer in owned_meta::unclaimed(input.raw, &[], &[]) {
            parts.push(incomplete(
                ContractOwner::RouterExtension,
                MissingContext::MissingOwner { pointer },
            ));
        }
        EdgeConformance::merged(parts)
    }
}

/// The Claude pin's `x-meta` places with the paths each definition fully checks (correction 5,
/// C4-1): a declared property is claimed whole when its schema is closed (`additionalProperties:
/// false`) or not an object; an open object claims only its declared children, at every depth, so
/// an undeclared key beneath it (such as `goal._meta.router`) is never covered by its ancestor.
fn with_declared_keys(mut pins: ClaudeMetaPins, defs: &Map<String, Value>) -> ClaudeMetaPins {
    let names: Vec<String> = pins.places.values().cloned().collect();
    for name in names {
        let mut found = Vec::new();
        if let Some(definition) = defs.get(&name) {
            claim_paths(defs, definition, "", &mut found, 0);
        }
        found.sort();
        found.dedup();
        pins.declared.insert(name, found);
    }
    pins
}

/// The parts a schema value is made of: itself, its `allOf`/`anyOf`/`oneOf` members and the
/// definitions it references, flattened.
fn schema_parts<'a>(
    defs: &'a Map<String, Value>,
    value: &'a Value,
    depth: usize,
) -> Vec<&'a Value> {
    if depth > 8 {
        return Vec::new();
    }
    let mut parts = vec![value];
    for keyword in ["allOf", "anyOf", "oneOf"] {
        for part in value
            .get(keyword)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            parts.extend(schema_parts(defs, part, depth + 1));
        }
    }
    if let Some(definition) = value
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| defs.get(name))
    {
        parts.extend(schema_parts(defs, definition, depth + 1));
    }
    parts
}

/// The claim paths (relative, RFC 6901 segments) a schema value fully checks, under `prefix`.
fn claim_paths(
    defs: &Map<String, Value>,
    value: &Value,
    prefix: &str,
    found: &mut Vec<String>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let parts = schema_parts(defs, value, 0);
    for part in &parts {
        for (name, schema) in part
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let path = if prefix.is_empty() {
                owned_meta::escaped(name)
            } else {
                format!("{prefix}/{}", owned_meta::escaped(name))
            };
            let property_parts = schema_parts(defs, schema, 0);
            let object = property_parts.iter().any(|part| {
                part.get("type") == Some(&json!("object")) || part.get("properties").is_some()
            });
            let closed = property_parts
                .iter()
                .any(|part| part.get("additionalProperties") == Some(&json!(false)));
            if object && !closed {
                claim_paths(defs, schema, &path, found, depth + 1);
            } else {
                found.push(path);
            }
        }
    }
}

/// A `_meta` place's verdict with its violations located in the frame.
fn at_pointer(conformance: EdgeConformance, pointer: &str) -> EdgeConformance {
    match conformance {
        EdgeConformance::Violations {
            owner,
            definition,
            violations,
        } => EdgeConformance::Violations {
            owner,
            definition,
            violations: violations
                .into_iter()
                .map(|mut violation| {
                    violation.instance_path = format!("{pointer}{}", violation.instance_path);
                    violation
                })
                .collect(),
        },
        other => other,
    }
}

fn incomplete(owner: ContractOwner, missing: MissingContext) -> EdgeConformance {
    EdgeConformance::Incomplete { owner, missing }
}

/// A judged frame, or `Incomplete` when its support could not be decided.
fn support_judged(
    owner: ContractOwner,
    name: &str,
    violations: Result<Vec<Violation>, String>,
    support: Result<Support, MissingContext>,
) -> EdgeConformance {
    match support {
        Ok(support) => judged(owner, name.to_owned(), violations, support),
        // A support decision that could not be made stays named beside any violation.
        Err(missing) => match violations {
            Ok(found) if found.is_empty() => incomplete(owner, missing),
            other => EdgeConformance::merged(vec![
                judged(owner, name.to_owned(), other, Support::NotApplicable),
                incomplete(owner, missing),
            ]),
        },
    }
}

/// Whether the agent negotiated the prompt content a client's `session/prompt` uses, by the
/// product's own rule (`provider_prompt_content.rs:82`): text and resource links always; images,
/// audio and embedded resources only when the agent's `promptCapabilities` advertise them.
fn prompt_support(
    method: &str,
    input: &CheckInput<'_>,
    ledger: &ConnectionLedger,
) -> Result<Support, MissingContext> {
    if method != "session/prompt" || input.sender != ProtocolRole::Client {
        return Ok(Support::NotApplicable);
    }
    let mut needed = Vec::new();
    for block in at(input.raw, &["params", "prompt"])
        .as_array()
        .into_iter()
        .flatten()
    {
        let capability = match at(block, &["type"]).as_str() {
            Some("image") => "image",
            Some("audio") => "audio",
            Some("resource") => "embeddedContext",
            _ => continue,
        };
        if !needed.contains(&capability) {
            needed.push(capability);
        }
    }
    if needed.is_empty() {
        return Ok(Support::NotApplicable);
    }
    let Some(negotiated) = ledger.negotiated(input.connection) else {
        return Err(MissingContext::NoNegotiation {
            capability: needed.join(","),
        });
    };
    let capabilities = &at(negotiated, &["agentCapabilities", "promptCapabilities"]);
    match needed
        .iter()
        .find(|capability| capabilities[**capability] != json!(true))
    {
        Some(capability) => Ok(Support::NotNegotiated {
            capability: (*capability).to_owned(),
        }),
        None => Ok(Support::Negotiated),
    }
}

/// Whether the Claude agent advertised the extension method a request uses, each by its own
/// advertisement in the `initialize` result `_meta`: `_session/steering` by
/// `steering.supported` (the flag `provider_capability_report.rs` reads); `_session/goal` by
/// `goal` (version 1, `controlMethod` naming the method, `actions` including the request's action).
fn claude_method_support(
    method: &str,
    input: &CheckInput<'_>,
    ledger: &ConnectionLedger,
) -> Result<Support, MissingContext> {
    let capability = match method {
        "_session/steering" => "steering",
        "_session/goal" => "goal",
        _ => return Ok(Support::NotApplicable),
    };
    let Some(negotiated) = ledger.negotiated(input.connection) else {
        return Err(MissingContext::NoNegotiation {
            capability: capability.to_owned(),
        });
    };
    let advertised = match method {
        "_session/steering" => at(negotiated, &["_meta", "steering", "supported"]) == &json!(true),
        _ => {
            let goal = at(negotiated, &["_meta", "goal"]);
            let action = at(input.raw, &["params", "action"]);
            at(goal, &["version"]) == &json!(1)
                && at(goal, &["controlMethod"]) == &json!(method)
                && at(goal, &["actions"])
                    .as_array()
                    .is_some_and(|actions| actions.contains(action))
        }
    };
    if advertised {
        Ok(Support::Negotiated)
    } else {
        Ok(Support::NotNegotiated {
            capability: capability.to_owned(),
        })
    }
}
