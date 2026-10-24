//! Cursor 2026.10.22 CustomCommandsLoader's metadata rules, without expansion.
use super::skill_document::split_skill_document;
use serde::Deserialize;
use serde_json::Value;

pub(super) struct CursorSkillMetadata {
    pub description: String,
    pub user_invocable: bool,
}

#[derive(Default, Deserialize)]
struct CursorFrontmatter {
    name: Option<Value>,
    description: Option<Value>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<Value>,
    metadata: Option<CursorMetadata>,
}

#[derive(Default, Deserialize)]
struct CursorMetadata {
    surfaces: Option<Value>,
}

pub(super) fn parse_cursor_skill(
    contents: &str,
    canonical_name: &str,
) -> Result<Option<CursorSkillMetadata>, ()> {
    if contents.trim().is_empty() {
        return Ok(None);
    }
    let document = split_skill_document(contents).map_err(|_| ())?;
    let header: CursorFrontmatter = match document.frontmatter {
        Some(frontmatter) => serde_saphyr::from_str_with_options(
            frontmatter,
            serde_saphyr::options! { strict_booleans: true, reject_unsupported_tags: true },
        )
        .map_err(|_| ())?,
        None => CursorFrontmatter::default(),
    };
    // Value occurs only at this native, dynamically typed file boundary. Native
    // Cursor accepts text fields only when their YAML value is actually text.
    let title = match header.name {
        Some(Value::String(value)) => value,
        _ => canonical_name.to_owned(),
    };
    let description = match header.description {
        Some(Value::String(value)) if !value.is_empty() => value,
        _ => body_description(document.body).unwrap_or_else(|| title.clone()),
    };
    let user_invocable = match header.user_invocable {
        Some(Value::Bool(false)) => false,
        Some(Value::String(value)) => value.trim().to_lowercase() != "false",
        _ => true,
    };
    let surfaces = match header.metadata.and_then(|metadata| metadata.surfaces) {
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Some(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    if !surfaces.is_empty() && !surfaces.iter().any(|surface| surface == "cli") {
        return Ok(None);
    }
    Ok(Some(CursorSkillMetadata {
        description,
        user_invocable,
    }))
}

fn body_description(body: &str) -> Option<String> {
    for line in body.lines() {
        let candidate = if line.starts_with('#') {
            line.trim_start_matches('#').trim()
        } else if !line.starts_with("---") {
            line.trim()
        } else {
            continue;
        };
        if !candidate.is_empty() {
            return Some(candidate.to_owned());
        }
    }
    None
}
