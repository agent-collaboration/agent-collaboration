//! MCP frames on N11 (Program Design "Who owns each contract"): the envelope against the `rmcp`
//! types the Router compiles (=3.9.0); a `tools/call`'s `arguments` against that tool's
//! advertised input schema; its `structuredContent` against the advertised output schema; and
//! each advertised schema as a JSON Schema document whose Codex parts (the `codexNative*`
//! definitions `schema_binding.rs` splices in) mean the same as the pinned Codex export's:
//! compared after normalizing equivalent spellings (numbers by value, so `0` equals `0.0`;
//! the boolean schemas `true` and `false` as `{}` and `{"not":{}}`). Every advertised-schema
//! violation names its tool's position in `tools`, so two tools' identical defects stay two.

use super::super::json_path::at;
use serde_json::{Value, json};

use super::super::connection_ledger::ConnectionLedger;
use super::{
    CheckInput, ContractOwner, EdgeConformance, EdgeDefinitions, MissingContext, ProtocolRole,
    RpcShape, Support, Violation, answered_method, bodiless_or_double_reply, compact, rpc_shape,
    violations_of,
};

const OWNER: ContractOwner = ContractOwner::Mcp;
const CALL_PREFIX: &str = "tools/call ";

fn decode_violation(role: ProtocolRole, error: &serde_json::Error) -> Violation {
    let side = match role {
        ProtocolRole::Client => "ClientJsonRpcMessage",
        ProtocolRole::Server => "ServerJsonRpcMessage",
    };
    Violation {
        schema_path: format!("rmcp::model::{side}"),
        instance_path: String::new(),
        keyword: "rmcpDecode".to_owned(),
        detail: String::new(),
        expected: format!("a frame rmcp 3.9.0 decodes as {side}"),
        found: error.to_string(),
    }
}

/// Decodes the frame with the rmcp type for its sender. rmcp's request and result unions end in
/// catch-all custom variants, so a malformed known method would still decode whole; the params
/// or result of each method N11 serves are therefore also decoded with that method's own type.
fn envelope_violations(role: ProtocolRole, raw: &Value, method: Option<&str>) -> Vec<Violation> {
    let whole = match role {
        ProtocolRole::Client => {
            serde_json::from_value::<rmcp::model::ClientJsonRpcMessage>(raw.clone()).map(|_| ())
        }
        ProtocolRole::Server => {
            serde_json::from_value::<rmcp::model::ServerJsonRpcMessage>(raw.clone()).map(|_| ())
        }
    };
    let body = |field: &str| raw.get(field).cloned().unwrap_or(Value::Null);
    let typed = match (role, method) {
        (ProtocolRole::Client, Some("initialize")) => {
            decode::<rmcp::model::InitializeRequestParams>(
                body("params"),
                "InitializeRequestParams",
            )
        }
        (ProtocolRole::Client, Some("tools/call")) => {
            decode::<rmcp::model::CallToolRequestParams>(body("params"), "CallToolRequestParams")
        }
        (ProtocolRole::Server, Some("initialize")) => {
            decode::<rmcp::model::InitializeResult>(body("result"), "InitializeResult")
        }
        (ProtocolRole::Server, Some("tools/list")) => {
            decode::<rmcp::model::ListToolsResult>(body("result"), "ListToolsResult")
        }
        (ProtocolRole::Server, Some(method)) if method.starts_with(CALL_PREFIX) => {
            decode::<rmcp::model::CallToolResult>(body("result"), "CallToolResult")
        }
        _ => None,
    };
    whole
        .err()
        .map(|error| decode_violation(role, &error))
        .into_iter()
        .chain(typed)
        .collect()
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, type_name: &str) -> Option<Violation> {
    serde_json::from_value::<T>(value)
        .err()
        .map(|error| Violation {
            schema_path: format!("rmcp::model::{type_name}"),
            instance_path: String::new(),
            keyword: "rmcpDecode".to_owned(),
            detail: String::new(),
            expected: format!("a value rmcp 3.9.0 decodes as {type_name}"),
            found: error.to_string(),
        })
}

/// Prefixes each violation's instance path with where the body sits in the frame.
fn prefixed(prefix: &str, violations: Vec<Violation>) -> Vec<Violation> {
    violations
        .into_iter()
        .map(|mut violation| {
            violation.instance_path = format!("{prefix}{}", violation.instance_path);
            violation
        })
        .collect()
}

/// Validates `instance` against an advertised schema, which must itself be a valid document.
fn against_advertised(
    definitions: &EdgeDefinitions,
    tool: &str,
    which: &str,
    schema: &Value,
    instance: &Value,
) -> Vec<Violation> {
    let key = format!("mcp:{tool}:{which}:{schema}");
    match definitions.validate(&key, || schema.clone(), instance) {
        Ok(violations) => violations,
        Err(problem) => vec![Violation {
            schema_path: format!("tools/{tool}/{which}"),
            instance_path: String::new(),
            keyword: "compile".to_owned(),
            detail: String::new(),
            expected: "an advertised schema the checker can compile".to_owned(),
            found: problem,
        }],
    }
}

/// Keywords whose value is one schema.
const SCHEMA_KEYWORDS: &[&str] = &[
    "additionalProperties",
    "additionalItems",
    "items",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
    "unevaluatedProperties",
    "unevaluatedItems",
];
/// Keywords whose value maps names to schemas.
const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];
/// Keywords whose value is a list of schemas.
const SCHEMA_LIST_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

/// A JSON number in one canonical form, so `0` and `0.0` (one number to JSON and JSON Schema)
/// compare equal.
///
/// Named limit (correction 4, V6): an integral float at or above 2^53 is not canonicalized, so
/// `9007199254740992` and `9007199254740992.0` compare as different numbers. No pinned Codex
/// literal reaches that range; the report names this limit.
fn normalized_number(number: &serde_json::Number) -> Value {
    if let Some(float) = number.as_f64()
        && float.fract() == 0.0
        && float.abs() < 9_007_199_254_740_992.0
    {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "An integral float below 2^53 converts to i64 exactly."
        )]
        return json!(float as i64);
    }
    Value::Number(number.clone())
}

/// A value with every number in canonical form (`const`, `enum`, `default` and the like).
fn normalized_value(value: &Value) -> Value {
    match value {
        Value::Number(number) => normalized_number(number),
        Value::Array(items) => Value::Array(items.iter().map(normalized_value).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, child)| (key.clone(), normalized_value(child)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A schema in canonical form: the boolean schemas `true` and `false` become `{}` and
/// `{"not":{}}` wherever a schema stands, and numbers are canonical everywhere. Two schemas that
/// differ only in these spellings accept exactly the same values.
fn normalized_schema(schema: &Value) -> Value {
    match schema {
        Value::Bool(true) => json!({}),
        Value::Bool(false) => json!({"not": {}}),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, child)| {
                    let normalized = if SCHEMA_KEYWORDS.contains(&key.as_str()) {
                        normalized_schema(child)
                    } else if SCHEMA_MAP_KEYWORDS.contains(&key.as_str()) {
                        match child {
                            Value::Object(entries) => Value::Object(
                                entries
                                    .iter()
                                    .map(|(name, entry)| (name.clone(), normalized_schema(entry)))
                                    .collect(),
                            ),
                            other => normalized_value(other),
                        }
                    } else if SCHEMA_LIST_KEYWORDS.contains(&key.as_str()) {
                        match child {
                            Value::Array(entries) => {
                                Value::Array(entries.iter().map(normalized_schema).collect())
                            }
                            other => normalized_schema(other),
                        }
                    } else {
                        normalized_value(child)
                    };
                    (key.clone(), normalized)
                })
                .collect(),
        ),
        other => normalized_value(other),
    }
}

/// Rewrites the export's `#/definitions/v2/X` references as `schema_binding.rs` does.
fn rebased(mut value: Value) -> Value {
    fn walk(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if let Some(Value::String(reference)) = fields.get_mut("$ref")
                    && let Some(name) = reference.strip_prefix("#/definitions/v2/")
                {
                    *reference = format!("#/$defs/codexNative{name}");
                }
                fields.values_mut().for_each(walk);
            }
            Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut value);
    value
}

fn local_refs(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            if let Some(name) = fields
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
            {
                found.push(name.to_owned());
            }
            fields.values().for_each(|child| local_refs(child, found));
        }
        Value::Array(items) => items.iter().for_each(|child| local_refs(child, found)),
        _ => {}
    }
}

/// One advertised schema as a document: valid JSON Schema, every local reference defined, and
/// every Codex part equal to the pinned export's definition.
fn advertised_schema_violations(
    definitions: &EdgeDefinitions,
    tool: &str,
    which: &str,
    schema: &Value,
) -> Vec<Violation> {
    let path = format!("tools/{tool}/{which}");
    let document = match jsonschema::meta::validator_for(schema) {
        Ok(meta) => violations_of(meta.iter_errors(schema)),
        Err(error) => vec![Violation {
            schema_path: path.clone(),
            instance_path: String::new(),
            keyword: "$schema".to_owned(),
            detail: String::new(),
            expected: "a schema whose draft the checker knows".to_owned(),
            found: error.to_string(),
        }],
    };
    let mut violations: Vec<Violation> = prefixed(&format!("/{which}"), document);
    let local = at(schema, &["$defs"]).as_object();
    let mut referenced = Vec::new();
    local_refs(schema, &mut referenced);
    for name in referenced {
        if local.is_none_or(|defs| !defs.contains_key(&name)) {
            violations.push(Violation {
                schema_path: path.clone(),
                instance_path: format!("/{which}/$defs/{name}"),
                keyword: "codexParts".to_owned(),
                detail: String::new(),
                expected: format!("the referenced definition {name}"),
                found: "nothing: the reference dangles".to_owned(),
            });
        }
    }
    for (name, definition) in local.into_iter().flatten() {
        let Some(codex_name) = name.strip_prefix("codexNative") else {
            continue;
        };
        let pinned = &definitions.codex_v2_definitions()[codex_name];
        let expected = rebased(pinned.clone());
        if pinned.is_null() || normalized_schema(&expected) != normalized_schema(definition) {
            violations.push(Violation {
                schema_path: format!("#/definitions/v2/{codex_name}"),
                instance_path: format!("/{which}/$defs/{name}"),
                keyword: "codexParts".to_owned(),
                detail: String::new(),
                expected: format!(
                "the pinned Codex 0.166.0 definition {codex_name}, rebased, or an equivalent spelling"
            ),
                found: compact(definition),
            });
        }
    }
    violations
}

pub(super) fn check(
    definitions: &EdgeDefinitions,
    input: &CheckInput<'_>,
    ledger: &mut ConnectionLedger,
) -> EdgeConformance {
    let Some(shape) = rpc_shape(input.raw) else {
        return EdgeConformance::Incomplete {
            owner: OWNER,
            missing: MissingContext::NotJsonRpc,
        };
    };
    let mut violations = Vec::new();
    let mut missing: Option<MissingContext> = None;
    let definition;
    let decoded_as: Option<String>;
    let duplicate_id = |violations: &mut Vec<Violation>,
                        duplicate: super::super::connection_ledger::DuplicateOutstandingId,
                        method: &str| {
        violations.push(Violation {
            schema_path: "jsonrpc/id".to_owned(),
            instance_path: "/id".to_owned(),
            keyword: "duplicateId".to_owned(),
            detail: method.to_owned(),
            expected: format!(
                "an id no request on this connection is still waiting on; {} is waiting on it",
                duplicate.first_method
            ),
            found: compact(&duplicate.id),
        });
    };
    match shape {
        RpcShape::Request { id, method } => {
            decoded_as = Some(method.to_owned());
            if method == "tools/call" {
                let tool = at(input.raw, &["params", "name"])
                    .as_str()
                    .unwrap_or_default();
                let call = format!("{CALL_PREFIX}{tool}");
                if let Err(duplicate) =
                    ledger.open_request(input.connection, input.sender, id, &call)
                {
                    duplicate_id(&mut violations, duplicate, &call);
                }
                // With no advertisement the arguments cannot be judged, but the envelope still is.
                match ledger.tool(input.connection, tool) {
                    Some(advertised) => {
                        let arguments = at(input.raw, &["params"])
                            .get("arguments")
                            .cloned()
                            .unwrap_or(json!({}));
                        violations.extend(prefixed(
                            "/params/arguments",
                            against_advertised(
                                definitions,
                                tool,
                                "inputSchema",
                                at(advertised, &["inputSchema"]),
                                &arguments,
                            ),
                        ));
                    }
                    None => {
                        missing = Some(MissingContext::NoAdvertisement {
                            tool: tool.to_owned(),
                        });
                    }
                }
                definition = call;
            } else {
                if let Err(duplicate) =
                    ledger.open_request(input.connection, input.sender, id, method)
                {
                    duplicate_id(&mut violations, duplicate, method);
                }
                definition = method.to_owned();
            }
        }
        RpcShape::Notification { method } => {
            decoded_as = None;
            definition = method.to_owned();
        }
        RpcShape::Success { id, result } => {
            let Some(method) = answered_method(input, ledger, id) else {
                return EdgeConformance::Incomplete {
                    owner: OWNER,
                    missing: MissingContext::OrphanReply { id: id.clone() },
                };
            };
            decoded_as = Some(method.clone());
            if method == "tools/list" {
                let tools = at(result, &["tools"])
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for (index, tool) in tools.iter().enumerate() {
                    let name = at(tool, &["name"]).as_str().unwrap_or_default();
                    for which in ["inputSchema", "outputSchema"] {
                        if let Some(schema) = tool.get(which) {
                            violations.extend(prefixed(
                                &format!("/result/tools/{index}"),
                                advertised_schema_violations(definitions, name, which, schema),
                            ));
                        }
                    }
                }
                ledger.record_tools(input.connection, &tools);
            } else if let Some(tool) = method.strip_prefix(CALL_PREFIX)
                && let Some(structured) = result.get("structuredContent")
            {
                match ledger
                    .tool(input.connection, tool)
                    .and_then(|tool| tool.get("outputSchema"))
                {
                    Some(schema) => violations.extend(prefixed(
                        "/result/structuredContent",
                        against_advertised(definitions, tool, "outputSchema", schema, structured),
                    )),
                    None => violations.push(Violation {
                        schema_path: format!("tools/{tool}/outputSchema"),
                        instance_path: "/result/structuredContent".to_owned(),
                        keyword: "outputSchema".to_owned(),
                        detail: String::new(),
                        expected:
                            "structured content only from a tool that advertises an output schema"
                                .to_owned(),
                        found: compact(structured),
                    }),
                }
            }
            definition = format!("{method} result");
        }
        RpcShape::BareReply { id } => {
            let Some(method) = answered_method(input, ledger, id) else {
                return EdgeConformance::Incomplete {
                    owner: OWNER,
                    missing: MissingContext::OrphanReply { id: id.clone() },
                };
            };
            if let EdgeConformance::Violations {
                violations: found, ..
            } = bodiless_or_double_reply(input.raw, OWNER, &method)
            {
                violations.extend(found);
            }
            decoded_as = None;
            definition = format!("{method} result");
        }
        RpcShape::Error { id } => {
            if answered_method(input, ledger, id).is_none() {
                return EdgeConformance::Incomplete {
                    owner: OWNER,
                    missing: MissingContext::OrphanReply { id: id.clone() },
                };
            }
            decoded_as = None;
            definition = "error".to_owned();
        }
    }
    violations.extend(envelope_violations(
        input.sender,
        input.raw,
        decoded_as.as_deref(),
    ));
    violations.sort();
    violations.dedup();
    let checked = if violations.is_empty() {
        EdgeConformance::Conforms {
            owner: OWNER,
            definition,
            support: Support::NotApplicable,
        }
    } else {
        EdgeConformance::Violations {
            owner: OWNER,
            definition,
            violations,
        }
    };
    match missing {
        None => checked,
        Some(missing) => EdgeConformance::merged(vec![
            checked,
            EdgeConformance::Incomplete {
                owner: OWNER,
                missing,
            },
        ]),
    }
}
