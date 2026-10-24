//! Classify malformed closed-union request fields without echoing untrusted input.
use super::invalid_field;
use message_board::*;
use serde_json::{Map, Value};

pub(super) fn validate_closed_variants(
    tool: &str,
    fields: &Map<String, Value>,
) -> Result<(), BoardError> {
    validate_variant::<PostTarget>(
        fields,
        "placement",
        "must reference an existing thread with its required UUIDv7 field; new roots use thread_create with about",
    )?;
    if tool == "discovery_search" || tool == "post_search" {
        if fields
            .get("query")
            .and_then(Value::as_str)
            .and_then(|value| SearchQuery::try_from(value.to_owned()).ok())
            .is_none()
        {
            return Err(invalid_field(
                "query",
                "must contain 1 to 256 UTF-8 bytes after trimming",
            ));
        }
        if !fields.contains_key("scope") {
            return Err(invalid_field("scope", "is required"));
        }
        if tool == "discovery_search" {
            validate_variant::<DiscoveryScope>(
                fields,
                "scope",
                "must select allWorkspaces, workspace, or board",
            )?;
            validate_variant::<DiscoveryKind>(
                fields,
                "kind",
                "must be all, workspace, board, or project",
            )?;
        } else {
            validate_variant::<PostListScope>(
                fields,
                "scope",
                "must select allWorkspaces, workspace, board, project, or thread",
            )?;
            validate_variant::<SearchPostKind>(
                fields,
                "kind",
                "must be both, topLevel, or thread",
            )?;
        }
    } else if tool == "post_list" {
        validate_variant::<PostListScope>(
            fields,
            "scope",
            "must be project, thread, board, workspace, or allWorkspaces",
        )?;
        validate_variant::<PostSelection>(
            fields,
            "selection",
            "must be latest, afterPosition, or an ordered inclusive range",
        )?;
    } else if tool == "inbox_fetch" {
        if !fields.contains_key("scope") {
            return Err(invalid_field(
                "scope",
                "is required and must select workspace, board, or project",
            ));
        }
        validate_variant::<InboxScope>(
            fields,
            "scope",
            "must be workspace, board, or project with its required UUIDv7 field",
        )?;
        validate_variant::<InboxReadMode>(fields, "readMode", "must be unread or latest")?;
    } else if tool == "inbox_acknowledge" {
        validate_variant::<ReadScopeRef>(
            fields,
            "scope",
            "must be project or thread with its required UUIDv7 field",
        )?;
    } else if tool == "thread_subscribe" {
        if !fields.contains_key("scope") {
            return Err(invalid_field("scope", "is required"));
        }
        validate_variant::<SubscriptionTarget>(
            fields,
            "scope",
            "must select thread or project with its required UUIDv7 field",
        )?;
        if !fields.contains_key("policy") {
            return Err(invalid_field("policy", "is required"));
        }
        validate_variant::<SubscriptionPolicyPatch>(
            fields,
            "policy",
            "must contain supported optional mode, idle, timing and lifetime fields",
        )?;
    } else if tool == "thread_unsubscribe" {
        if !fields.contains_key("scope") {
            return Err(invalid_field("scope", "is required"));
        }
        validate_variant::<SubscriptionTarget>(
            fields,
            "scope",
            "must select thread or project with its required UUIDv7 field",
        )?;
    }
    if let Some(references) = fields.get("references")
        && serde_json::from_value::<PostReferences>(references.clone()).is_err()
    {
        return Err(invalid_field(
            "references",
            "must contain at most 64 distinct message or thread targets",
        ));
    }
    Ok(())
}

fn validate_variant<TValue: serde::de::DeserializeOwned>(
    fields: &Map<String, Value>,
    field: &'static str,
    requirement: &'static str,
) -> Result<(), BoardError> {
    if let Some(value) = fields.get(field)
        && serde_json::from_value::<TValue>(value.clone()).is_err()
    {
        return Err(invalid_field(field, requirement));
    }
    Ok(())
}
