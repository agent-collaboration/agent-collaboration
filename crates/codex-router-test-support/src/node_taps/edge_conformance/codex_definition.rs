//! Codex app-server frames against the pinned 0.166.0 `--experimental` export and the pairing
//! transcribed from `common.rs` at `rust-v0.166.0`.
//!
//! A request or notification is checked whole against its own variant of the exported
//! `ClientRequest`, `ServerRequest`, `ClientNotification` or `ServerNotification` union, so its
//! envelope and its params are both judged. A success reply is checked against the response
//! type the pairing declares for the request it answers; an error reply against `JSONRPCError`.

use super::super::json_path::at;
use std::collections::HashMap;

use serde_json::{Value, json};

use super::super::connection_ledger::ConnectionLedger;
use super::{
    CheckInput, ContractOwner, EdgeConformance, EdgeDefinitions, MissingContext, ProtocolRole,
    RpcShape, Support, answered_method, bodiless_or_double_reply, judged, rpc_shape,
    unknown_method_violation, with_request_opened,
};

pub(super) struct CodexDefinition {
    bundle: Value,
    /// `(sender, method)` to the JSON pointer of that request's variant in its union.
    requests: HashMap<(Role, String), String>,
    notifications: HashMap<(Role, String), String>,
    /// `(request sender, method)` to the response definition's JSON pointer.
    responses: HashMap<(Role, String), String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Role {
    Client,
    Server,
}

impl From<ProtocolRole> for Role {
    fn from(role: ProtocolRole) -> Self {
        match role {
            ProtocolRole::Client => Self::Client,
            ProtocolRole::Server => Self::Server,
        }
    }
}

/// Indexes each variant of the union `name` by its single `method` enum value.
fn union_variants(bundle: &Value, name: &str) -> Result<HashMap<String, String>, String> {
    let variants = at(bundle, &["definitions", name, "oneOf"])
        .as_array()
        .ok_or_else(|| format!("the bundle has no {name} union"))?;
    let mut indexed = HashMap::new();
    for (index, variant) in variants.iter().enumerate() {
        let method = at(variant, &["properties", "method", "enum"])[0]
            .as_str()
            .ok_or_else(|| format!("{name} variant {index} has no method"))?;
        indexed.insert(
            method.to_owned(),
            format!("#/definitions/{name}/oneOf/{index}"),
        );
    }
    Ok(indexed)
}

const OWNER: ContractOwner = ContractOwner::Codex;

impl CodexDefinition {
    pub(super) fn new(bundle: Value, pairing: &Value) -> Result<Self, String> {
        let mut requests = HashMap::new();
        let mut notifications = HashMap::new();
        for (sender, request_union, notification_union) in [
            (Role::Client, "ClientRequest", "ClientNotification"),
            (Role::Server, "ServerRequest", "ServerNotification"),
        ] {
            for (method, pointer) in union_variants(&bundle, request_union)? {
                requests.insert((sender, method), pointer);
            }
            for (method, pointer) in union_variants(&bundle, notification_union)? {
                notifications.insert((sender, method), pointer);
            }
        }
        let mut responses = HashMap::new();
        for (sender, list) in [
            (Role::Client, "clientRequests"),
            (Role::Server, "serverRequests"),
        ] {
            for request in pairing[list]
                .as_array()
                .ok_or_else(|| format!("the pairing has no {list}"))?
            {
                if at(request, &["bundle", "kind"]) != "exported" {
                    continue;
                }
                let (Some(method), Some(response)) = (
                    at(request, &["method"]).as_str(),
                    at(request, &["bundle", "responseDefinition"]).as_str(),
                ) else {
                    return Err(format!("a malformed {list} entry: {request}"));
                };
                if !requests.contains_key(&(sender, method.to_owned())) {
                    return Err(format!(
                        "the pairing pairs {method}, which the bundle does not export"
                    ));
                }
                responses.insert((sender, method.to_owned()), response.to_owned());
            }
        }
        if responses.len() != requests.len() {
            return Err(format!(
                "the pairing pairs {} requests, the bundle exports {}",
                responses.len(),
                requests.len()
            ));
        }
        Ok(Self {
            bundle,
            requests,
            notifications,
            responses,
        })
    }

    pub(super) fn v2_definitions(&self) -> &Value {
        at(&self.bundle, &["definitions", "v2"])
    }

    fn judge(
        &self,
        definitions: &EdgeDefinitions,
        pointer: &str,
        instance: &Value,
    ) -> EdgeConformance {
        let schema = || {
            json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "$ref": pointer,
                "definitions": at(&self.bundle, &["definitions"]),
            })
        };
        judged(
            OWNER,
            pointer.to_owned(),
            definitions.validate(&format!("codex:{pointer}"), schema, instance),
            Support::NotApplicable,
        )
    }

    pub(super) fn check(
        &self,
        definitions: &EdgeDefinitions,
        input: &CheckInput<'_>,
        ledger: &mut ConnectionLedger,
    ) -> EdgeConformance {
        let role = Role::from(input.sender);
        let Some(shape) = rpc_shape(input.raw) else {
            return EdgeConformance::Incomplete {
                owner: OWNER,
                missing: MissingContext::NotJsonRpc,
            };
        };
        match shape {
            RpcShape::Request { id, method } => {
                let verdict = match self.requests.get(&(role, method.to_owned())) {
                    Some(pointer) => self.judge(definitions, pointer, input.raw),
                    None => unknown(role, method, "request"),
                };
                with_request_opened(ledger, input, OWNER, id, method, verdict)
            }
            RpcShape::Notification { method } => {
                match self.notifications.get(&(role, method.to_owned())) {
                    Some(pointer) => self.judge(definitions, pointer, input.raw),
                    None => unknown(role, method, "notification"),
                }
            }
            RpcShape::Success { id, result } => {
                let Some(method) = answered_method(input, ledger, id) else {
                    return orphan(id);
                };
                let request_sender = Role::from(input.sender.other());
                match self.responses.get(&(request_sender, method.clone())) {
                    Some(pointer) => self.judge(definitions, pointer, result),
                    None => EdgeConformance::Incomplete {
                        owner: OWNER,
                        missing: MissingContext::NoPairing { method },
                    },
                }
            }
            RpcShape::BareReply { id } => match answered_method(input, ledger, id) {
                Some(method) => bodiless_or_double_reply(input.raw, OWNER, &method),
                None => orphan(id),
            },
            RpcShape::Error { id } => {
                if answered_method(input, ledger, id).is_none() {
                    return orphan(id);
                }
                self.judge(definitions, "#/definitions/JSONRPCError", input.raw)
            }
        }
    }
}

fn unknown(sender: Role, method: &str, what: &str) -> EdgeConformance {
    let union = match (sender, what) {
        (Role::Client, "notification") => "ClientNotification",
        (Role::Server, "notification") => "ServerNotification",
        (Role::Client, _) => "ClientRequest",
        (Role::Server, _) => "ServerRequest",
    };
    EdgeConformance::Violations {
        owner: OWNER,
        definition: format!("#/definitions/{union}"),
        violations: vec![unknown_method_violation(
            &format!("/definitions/{union}/oneOf"),
            method,
            what,
        )],
    }
}

fn orphan(id: &Value) -> EdgeConformance {
    EdgeConformance::Incomplete {
        owner: OWNER,
        missing: MissingContext::OrphanReply { id: id.clone() },
    }
}
