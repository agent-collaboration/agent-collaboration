//! Decode the existing Claude 2.1.310 `plugin list --json` export.
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudePluginExportScope {
    User,
    Project,
    Local,
    Managed,
    Session,
    Synced,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginExport {
    pub id: String,
    pub version: String,
    pub scope: ClaudePluginExportScope,
    pub enabled: bool,
    pub install_path: PathBuf,
    pub read_from_folder: Option<PathBuf>,
    pub folder_version: Option<String>,
    pub project_path: Option<PathBuf>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub mcp_servers: Option<std::collections::BTreeMap<String, serde::de::IgnoredAny>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClaudePluginExportError {
    #[error("Claude native plugin export version is unqualified")]
    UnsupportedVersion,
    #[error("Claude native plugin export is invalid or unrepresentable")]
    InvalidExport,
}

pub fn read_claude_plugin_export(
    version: &str,
    bytes: &[u8],
) -> Result<Vec<ClaudePluginExport>, ClaudePluginExportError> {
    if version != "2.1.310" {
        return Err(ClaudePluginExportError::UnsupportedVersion);
    }
    serde_json::from_slice(bytes).map_err(|_| ClaudePluginExportError::InvalidExport)
}

/// This is the qualified regular native package key format, not a consumer ID
/// heuristic. npm/synced/session keys use different native formats and fail here.
pub fn regular_claude_plugin_key(value: &str) -> Result<(&str, &str), ClaudePluginExportError> {
    let mut parts = value.split('@');
    let name = parts.next().ok_or(ClaudePluginExportError::InvalidExport)?;
    let marketplace = parts.next().ok_or(ClaudePluginExportError::InvalidExport)?;
    if parts.next().is_some()
        || !regular_key_part(name)
        || !regular_key_part(marketplace)
        || marketplace.eq_ignore_ascii_case("npm")
    {
        return Err(ClaudePluginExportError::InvalidExport);
    }
    Ok((name, marketplace))
}

fn regular_key_part(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capabilities_plugin_export_keeps_package_scope_and_native_folder_override() {
        let bytes = br#"[{"id":"alpha@marketplace","version":"2.0","scope":"project","enabled":false,"installPath":"/native/installed","readFromFolder":"/native/active","projectPath":"/workspace","mcpServers":{"native-mcp":{"type":"http","url":"https://example.invalid/mcp"}}}]"#;
        let packages = read_claude_plugin_export("2.1.310", bytes).expect("native export");
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].scope, ClaudePluginExportScope::Project);
        assert!(!packages[0].enabled);
        assert_eq!(
            packages[0].read_from_folder.as_deref(),
            Some(std::path::Path::new("/native/active"))
        );
        assert_eq!(
            regular_claude_plugin_key(&packages[0].id),
            Ok(("alpha", "marketplace"))
        );
    }

    #[test]
    fn native_capabilities_plugin_export_never_defaults_missing_enablement_or_guesses_special_keys()
    {
        assert!(
            read_claude_plugin_export(
                "2.1.310",
                br#"[{"id":"alpha@market","version":"1","scope":"user","installPath":"/native"}]"#
            )
            .is_err()
        );
        for key in [
            "alpha",
            "alpha@npm",
            "alpha@market@extra",
            "@package@npm",
            "session[/native]",
            "alpha@",
        ] {
            assert!(regular_claude_plugin_key(key).is_err());
        }
    }
}
