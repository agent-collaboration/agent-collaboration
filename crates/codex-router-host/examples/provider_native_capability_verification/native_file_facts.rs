//! Independent facts read from the provider's own files: frontmatter flags, the
//! skill-source role, plugin manifests and marketplace manifests. These are the
//! native reference, read without the candidate.
use crate::verification_models::{NativePluginFacts, NativePluginRow, ProviderEndpoint};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// A scalar frontmatter value as written: quoted text keeps its inner characters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrontmatterScalar {
    Plain(String),
    Quoted(String),
    /// A block (`|`, `>`) or otherwise multi-line value this reader does not decode.
    Unobserved,
}

impl FrontmatterScalar {
    fn text(&self) -> Option<&str> {
        match self {
            Self::Plain(text) | Self::Quoted(text) => Some(text),
            Self::Unobserved => None,
        }
    }
}

/// Top-level single-line `key: value` fields of a SKILL.md or legacy command header.
pub(crate) fn frontmatter_fields(skill_path: &Path) -> BTreeMap<String, FrontmatterScalar> {
    let file = skill_source_file(skill_path);
    let Ok(contents) = fs::read_to_string(&file) else {
        return BTreeMap::new();
    };
    let contents = contents.trim_start_matches('\u{feff}');
    let mut lines = contents.lines().map(|line| line.trim_end_matches('\r'));
    if lines.next().map(str::trim) != Some("---") {
        return BTreeMap::new();
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };
        let raw = raw.trim();
        let quoted = ['\'', '"'].into_iter().find_map(|quote| {
            raw.strip_prefix(quote)
                .and_then(|inner| inner.strip_suffix(quote))
        });
        let value = match quoted {
            Some(inner) => FrontmatterScalar::Quoted(inner.to_owned()),
            None if raw.is_empty() || raw.starts_with(['|', '>']) => FrontmatterScalar::Unobserved,
            None => FrontmatterScalar::Plain(raw.to_owned()),
        };
        fields.insert(key.trim().to_owned(), value);
    }
    fields
}

/// Whether the provider's own loader treats this skill file as not user-invocable.
///
/// Cursor's CustomCommandsLoader skips `user-invocable` when it is boolean false or a
/// string that trims and lowercases to "false". Claude 2.1.310 also reads `no`, `off`
/// and `0` as false. Quoted values keep inner whitespace, so it is trimmed here too.
pub(crate) fn natively_disabled(skill_path: &Path, provider: ProviderEndpoint) -> bool {
    let fields = frontmatter_fields(skill_path);
    let Some(flag) = fields
        .get("user-invocable")
        .and_then(FrontmatterScalar::text)
    else {
        return false;
    };
    let normalized = flag.trim().to_ascii_lowercase();
    match provider {
        ProviderEndpoint::CursorLocal => normalized == "false",
        ProviderEndpoint::ClaudeLocal => {
            matches!(normalized.as_str(), "false" | "no" | "off" | "0")
        }
    }
}

/// The single-line `description`, unquoted and trimmed; None when not one line.
pub(crate) fn frontmatter_description(skill_path: &Path) -> Option<String> {
    match frontmatter_fields(skill_path).get("description")? {
        FrontmatterScalar::Plain(text) | FrontmatterScalar::Quoted(text) => {
            Some(text.trim().to_owned())
        }
        FrontmatterScalar::Unobserved => None,
    }
}

fn skill_source_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_owned()
    }
}

/// Program Design `SkillSource`: a `SKILL.md` (or its folder) under a skill root, or a
/// legacy command `.md` under a command root. A package folder, a marketplace manifest
/// or any other real path is the wrong role.
pub(crate) fn is_skill_source(
    path: &Path,
    skill_roots: &[PathBuf],
    command_roots: &[PathBuf],
) -> bool {
    if !path.is_absolute() || !path.exists() {
        return false;
    }
    let within = |roots: &[PathBuf]| {
        let resolved = canonical(path);
        roots
            .iter()
            .any(|root| resolved.starts_with(canonical(root)))
    };
    let skill_file =
        path.file_name().is_some_and(|name| name == "SKILL.md") || path.join("SKILL.md").is_file();
    let legacy_command = path.is_file() && path.extension().is_some_and(|ext| ext == "md");
    (skill_file && within(skill_roots)) || (legacy_command && within(command_roots))
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    canonical(left) == canonical(right)
}

/// The plugin's own skill files, from its active folder and its manifest's `skills`
/// roots (default `skills`): one `SKILL.md` per child folder.
pub(crate) fn plugin_skill_files(row: &NativePluginRow) -> Vec<PathBuf> {
    let root = row.active_root();
    let manifest = read_json(&root.join(".claude-plugin/plugin.json")).unwrap_or(Value::Null);
    let declared = match manifest.get("skills") {
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(paths)) => paths
            .iter()
            .filter_map(|path| path.as_str().map(str::to_owned))
            .collect(),
        _ => vec!["skills".to_owned()],
    };
    let mut files = Vec::new();
    for relative in declared {
        let Ok(children) = fs::read_dir(root.join(relative.trim_start_matches("./"))) else {
            continue;
        };
        for child in children.flatten() {
            let file = child.path().join("SKILL.md");
            if file.is_file() {
                files.push(file);
            }
        }
    }
    files.sort();
    files
}

/// The namespace the provider advertises a plugin's skills under (`namespace:skill`):
/// its manifest `name`, else the name part of an ordinary `name@marketplace` key.
pub(crate) fn plugin_namespace(row: &NativePluginRow) -> Option<String> {
    read_json(&row.active_root().join(".claude-plugin/plugin.json"))
        .and_then(|manifest| manifest.get("name")?.as_str().map(str::to_owned))
        .or_else(|| row.regular_key().map(|(name, _)| name.to_owned()))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Names a plugin declares under `key`, from its manifest (object or relative path)
/// or its own default file. No declaration is a known-empty list; an unreadable or
/// non-object declaration is unobserved (None).
fn declared_names(
    root: &Path,
    manifest: &Value,
    key: &str,
    default_file: &str,
) -> Option<Vec<String>> {
    let source = match manifest.get(key) {
        Some(object @ Value::Object(_)) => object.clone(),
        Some(Value::String(relative)) => read_json(&root.join(relative.trim_start_matches("./")))?,
        Some(_) => return None,
        None if root.join(default_file).exists() => read_json(&root.join(default_file))?,
        None => return Some(Vec::new()),
    };
    let names = match source.get(key) {
        Some(Value::Object(nested)) => nested.keys().cloned().collect(),
        _ => source.as_object()?.keys().cloned().collect(),
    };
    Some(names)
}

/// Per-item facts from one installed plugin's own files, retaining names only.
pub(crate) fn native_plugin_facts(row: &NativePluginRow) -> Result<NativePluginFacts, String> {
    let root = row.active_root();
    let manifest_file = root.join(".claude-plugin/plugin.json");
    let manifest = if manifest_file.exists() {
        read_json(&manifest_file).ok_or("plugin manifest is not JSON")?
    } else {
        Value::Null
    };
    let mut skill_descriptions = BTreeMap::new();
    for file in plugin_skill_files(row) {
        let name = file
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or("plugin skill has no folder name")?;
        let description = frontmatter_description(&file).ok_or_else(|| {
            format!(
                "native skill {} description is not one line",
                file.display()
            )
        })?;
        skill_descriptions.insert(name, description);
    }
    Ok(NativePluginFacts {
        plugin_id: row.id.clone(),
        skill_descriptions,
        hook_events: declared_names(root, &manifest, "hooks", "hooks/hooks.json"),
        mcp_servers: declared_names(root, &manifest, "mcpServers", ".mcp.json"),
    })
}

/// Native marketplace manifest resolution: a manifest file is itself; a marketplace
/// folder resolves to its `.claude-plugin/marketplace.json` when present.
pub(crate) fn marketplace_manifest(install_location: &Path) -> Option<PathBuf> {
    if !install_location.is_absolute() {
        return None;
    }
    if install_location.is_file() {
        return Some(install_location.to_owned());
    }
    let nested = install_location.join(".claude-plugin/marketplace.json");
    nested.is_file().then_some(nested)
}
