//! Owned `_meta` on captured ACP frames (D0 R52; correction 3, item B).
//!
//! ACP leaves `_meta` open, so the standard ACP check never judges it. Each `_meta` place a known
//! owner declares is selected here and held to that owner, alongside the standard check:
//!
//! | Edge and frame | Pointer | Owner |
//! |---|---|---|
//! | N8 or N9 serves an `initialize` result | `/result/_meta` | `InitializeProfileMetadata` |
//! | N9 serves a `session/new`, `session/load` or `session/resume` result | `/result/_meta` | `SessionResponseProfileMetadata` |
//! | a Claude agent's `initialize` result | `/result/_meta` | the Claude pin's `x-meta.initializeResponse` |
//! | a Claude agent's `session_info_update` | `/params/update/_meta` | the Claude pin's `x-meta.sessionInfoUpdate` |
//! | a Claude agent's result carrying `codexRouter.modelReasoning` | `/result/_meta/codexRouter/modelReasoning` | the Zod schema vs the Rust decoder |
//! | a Claude agent's `config_option_update` carrying it (model-metadata.ts:130 decorates both) | `/params/update/_meta/codexRouter/modelReasoning` | the Zod schema vs the Rust decoder |
//! | a Claude agent's `available_commands_update` | `/params/update/_meta/codexRouter` | the skill list annotation reader |
//! | each advertised command of that update | `/params/update/availableCommands/<i>/_meta/codexRouter` | the skill command annotation reader |
//!
//! Coverage is per key, never by ancestor (correction 4, V1): each check names the places its
//! owner fully checks (`claims`). A whole-object owner that denies unknown fields claims its whole
//! place; a Claude pin definition, an open object, claims only the properties it declares; the
//! skill readers claim only the key they read (`skillStatus`, `kind`). Every Router-owned key
//! (`router`, `sessionProfile`, `codexRouter`, `codex-router/*`) found in any `_meta` that no claim
//! covers is reported as missing its owner: never a silent pass.

use std::collections::HashMap;

use serde_json::Value;

use super::super::connection_ledger::AcpPeer;
use super::OutsideEdge;
use super::router_extension::RouterContract;

/// Where a frame sits in its exchange, as the owner table needs it.
#[derive(Clone, Copy, Debug)]
pub(super) enum FramePlace<'a> {
    Request,
    Notification { method: &'a str },
    Result { method: &'a str },
    Error,
}

/// Who a selected `_meta` place is held to.
#[derive(Clone, Debug)]
pub(super) enum MetaOwner {
    /// A Router-owned contract, checked by its registered owning code.
    Router(RouterContract),
    /// A definition in the Claude extras pin (`$defs/<name>`).
    ClaudePin(String),
}

/// One `_meta` place and its owner.
#[derive(Clone, Debug)]
pub(super) struct MetaCheck {
    /// An RFC 6901 pointer into the frame: the value the owner is given.
    pub pointer: String,
    pub owner: MetaOwner,
    /// The places the owner fully checks: its whole value, or only the keys it declares or reads.
    pub claims: Vec<String>,
}

/// The Claude pin's `x-meta`: each place's definition and the properties that definition declares.
#[derive(Clone, Debug, Default)]
pub(super) struct ClaudeMetaPins {
    /// Place name (`initializeResponse`, `sessionInfoUpdate`) to its definition.
    pub places: HashMap<String, String>,
    /// Definition name to the relative paths it fully checks: closed or non-object properties
    /// whole, and the declared children of open objects, at every depth (correction 5, C4-1).
    pub declared: HashMap<String, Vec<String>>,
}

/// Router-owned key names inside a `_meta` object.
fn is_router_key(key: &str) -> bool {
    matches!(key, "router" | "sessionProfile" | "codexRouter") || key.starts_with("codex-router/")
}

/// One RFC 6901 pointer segment.
pub(super) fn escaped(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// The owner table: every selected place that exists in `raw`.
pub(super) fn selected(
    edge: OutsideEdge,
    place: FramePlace<'_>,
    peer: Option<AcpPeer>,
    raw: &Value,
    claude_meta: &ClaudeMetaPins,
) -> Vec<MetaCheck> {
    let mut checks = Vec::new();
    let mut select = |pointer: String, owner: MetaOwner| {
        if raw.pointer(&pointer).is_none() {
            return;
        }
        let claims = match &owner {
            MetaOwner::Router(RouterContract::SkillListMeta) => {
                vec![format!("{pointer}/skillStatus")]
            }
            MetaOwner::Router(RouterContract::SkillCommandMeta) => vec![format!("{pointer}/kind")],
            MetaOwner::Router(_) => vec![pointer.clone()],
            MetaOwner::ClaudePin(name) => claude_meta
                .declared
                .get(name)
                .into_iter()
                .flatten()
                .map(|path| format!("{pointer}/{path}"))
                .collect(),
        };
        checks.push(MetaCheck {
            pointer,
            owner,
            claims,
        });
    };
    let claude_pin = |key: &str| {
        claude_meta
            .places
            .get(key)
            .cloned()
            .map(MetaOwner::ClaudePin)
    };
    match (edge, place) {
        (
            OutsideEdge::AcpServedN8 | OutsideEdge::AcpServedN9,
            FramePlace::Result {
                method: "initialize",
            },
        ) => {
            select(
                "/result/_meta".to_owned(),
                MetaOwner::Router(RouterContract::InitializeMeta),
            );
        }
        (
            OutsideEdge::AcpServedN9,
            FramePlace::Result {
                method: "session/new" | "session/load" | "session/resume",
            },
        ) => {
            select(
                "/result/_meta".to_owned(),
                MetaOwner::Router(RouterContract::SessionResultMeta),
            );
        }
        (OutsideEdge::AcpAgent, FramePlace::Result { method }) if peer == Some(AcpPeer::Claude) => {
            if method == "initialize"
                && let Some(owner) = claude_pin("initializeResponse")
            {
                select("/result/_meta".to_owned(), owner);
            }
            select(
                "/result/_meta/codexRouter/modelReasoning".to_owned(),
                MetaOwner::Router(RouterContract::ModelReasoningMeta),
            );
        }
        (OutsideEdge::AcpAgent, FramePlace::Notification { method })
            if peer == Some(AcpPeer::Claude) && method == "session/update" =>
        {
            match raw
                .pointer("/params/update/sessionUpdate")
                .and_then(Value::as_str)
            {
                Some("session_info_update") => {
                    if let Some(owner) = claude_pin("sessionInfoUpdate") {
                        select("/params/update/_meta".to_owned(), owner);
                    }
                }
                Some("config_option_update") => {
                    select(
                        "/params/update/_meta/codexRouter/modelReasoning".to_owned(),
                        MetaOwner::Router(RouterContract::ModelReasoningMeta),
                    );
                }
                Some("available_commands_update") => {
                    select(
                        "/params/update/_meta/codexRouter".to_owned(),
                        MetaOwner::Router(RouterContract::SkillListMeta),
                    );
                    let commands = raw
                        .pointer("/params/update/availableCommands")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    for index in 0..commands {
                        select(
                            format!("/params/update/availableCommands/{index}/_meta/codexRouter"),
                            MetaOwner::Router(RouterContract::SkillCommandMeta),
                        );
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    checks
}

/// Whether `pointer` (holding `value`) is covered by a claim: the place itself or an ancestor is
/// claimed, or every child of an object is.
fn covered(pointer: &str, value: &Value, checked: &[&str]) -> bool {
    if checked
        .iter()
        .any(|root| pointer == *root || pointer.starts_with(&format!("{root}/")))
    {
        return true;
    }
    let prefix = format!("{pointer}/");
    match value {
        Value::Object(fields)
            if !fields.is_empty() && checked.iter().any(|root| root.starts_with(&prefix)) =>
        {
            fields
                .iter()
                .all(|(key, child)| covered(&format!("{prefix}{}", escaped(key)), child, checked))
        }
        _ => false,
    }
}

/// Every Router-owned `_meta` key in `raw` that neither a selected check nor a claimed root
/// (a body a Router codec already owns whole) covers, as pointers.
pub(super) fn unclaimed(raw: &Value, checks: &[MetaCheck], claimed_roots: &[&str]) -> Vec<String> {
    let mut checked: Vec<&str> = checks
        .iter()
        .flat_map(|check| check.claims.iter().map(String::as_str))
        .collect();
    checked.extend_from_slice(claimed_roots);
    let mut found = Vec::new();
    walk(raw, "", &checked, &mut found);
    found
}

/// The places under `pointer` no claim covers: the place itself when nothing under it is claimed,
/// otherwise each uncovered child, so a report names the exact key that lacks an owner.
fn uncovered(pointer: &str, value: &Value, checked: &[&str], found: &mut Vec<String>) {
    if covered(pointer, value, checked) {
        return;
    }
    let prefix = format!("{pointer}/");
    match value {
        Value::Object(fields)
            if !fields.is_empty() && checked.iter().any(|root| root.starts_with(&prefix)) =>
        {
            for (key, child) in fields {
                uncovered(&format!("{prefix}{}", escaped(key)), child, checked, found);
            }
        }
        _ => found.push(pointer.to_owned()),
    }
}

fn walk(value: &Value, pointer: &str, checked: &[&str], found: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                let child_pointer = format!("{pointer}/{}", escaped(key));
                if key == "_meta"
                    && let Value::Object(meta) = child
                {
                    for (meta_key, meta_value) in meta {
                        let meta_pointer = format!("{child_pointer}/{}", escaped(meta_key));
                        if is_router_key(meta_key) {
                            uncovered(&meta_pointer, meta_value, checked, found);
                        }
                    }
                }
                walk(child, &child_pointer, checked, found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &format!("{pointer}/{index}"), checked, found);
            }
        }
        _ => {}
    }
}
