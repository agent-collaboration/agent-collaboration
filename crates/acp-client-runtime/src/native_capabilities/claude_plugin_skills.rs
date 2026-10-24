//! Qualified native plugin skill roots use different names from ordinary roots.
use super::{
    ClaudeFileSkill, ClaudeFileSkillScope, ClaudeFilesystemReadError, read_claude_skill_metadata,
};
use super::{
    claude_filesystem::{native_body_description, normalise_claude_description},
    skill_document::split_skill_document,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

/// The setup reader must first establish the native plugin name, actual root
/// order, configured enablement and absence of unqualified synced-wrapper gates.
pub fn read_claude_plugin_skill_roots(
    version: &str,
    plugin_name: &str,
    roots: &[PathBuf],
    scope: ClaudeFileSkillScope,
    enabled: bool,
    cancelled: &CancellationToken,
) -> Result<Vec<ClaudeFileSkill>, ClaudeFilesystemReadError> {
    if version != "2.1.310" {
        return Err(ClaudeFilesystemReadError::UnsupportedVersion);
    }
    if plugin_name.is_empty()
        || !plugin_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ClaudeFilesystemReadError::UnqualifiedSource);
    }
    let mut seen_names = HashSet::new();
    let mut seen_origins = HashSet::new();
    let mut skills = Vec::new();
    for root in roots {
        if cancelled.is_cancelled() {
            return Err(ClaudeFilesystemReadError::Cancelled);
        }
        if !root.is_absolute() {
            return Err(ClaudeFilesystemReadError::InvalidScope);
        }
        let direct = root.join("SKILL.md");
        let candidates = if direct
            .try_exists()
            .map_err(|_| ClaudeFilesystemReadError::Unreadable)?
        {
            vec![(direct, true)]
        } else {
            let entries = match fs::read_dir(root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(ClaudeFilesystemReadError::Unreadable),
            };
            let mut candidates = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
                let path = entry.path().join("SKILL.md");
                let kind = entry
                    .file_type()
                    .map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
                if (kind.is_dir() || kind.is_symlink())
                    && path
                        .try_exists()
                        .map_err(|_| ClaudeFilesystemReadError::Unreadable)?
                {
                    candidates.push((path, false));
                }
            }
            candidates
        };
        for (path, direct) in candidates {
            if cancelled.is_cancelled() {
                return Err(ClaudeFilesystemReadError::Cancelled);
            }
            let file = fs::metadata(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            if !file.is_file() || file.len() > 1_048_576 {
                return Err(ClaudeFilesystemReadError::InvalidMetadata);
            }
            let contents = super::skill_file::read_native_skill_file(&path, 1_048_576)
                .map_err(super::claude_filesystem::map_skill_file_error)?;
            let metadata = read_claude_skill_metadata(&contents)
                .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?;
            let folder = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .ok_or(ClaudeFilesystemReadError::InvalidMetadata)?;
            let explicit = if direct {
                metadata
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
            } else {
                None
            };
            let prefix = format!("{plugin_name}:");
            let bare = match explicit {
                Some(name) => {
                    let name = name.strip_prefix(&prefix).unwrap_or(name);
                    if name.is_empty() { folder } else { name }
                }
                None => folder,
            };
            if bare.contains('\u{feff}') {
                return Err(ClaudeFilesystemReadError::UnqualifiedSource);
            }
            let slug = bare
                .encode_utf16()
                .map(|unit| {
                    let character = char::from_u32(u32::from(unit));
                    match character {
                        Some(character)
                            if character.is_ascii_alphanumeric()
                                || matches!(character, '_' | '-') =>
                        {
                            character
                        }
                        _ => '-',
                    }
                })
                .collect::<String>();
            let name = format!("{plugin_name}:{slug}");
            let real_path =
                fs::canonicalize(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            if !seen_origins.insert(real_path) || !seen_names.insert(name.clone()) {
                return Err(ClaudeFilesystemReadError::AmbiguousOrigin);
            }
            if metadata.fallback == Some(true) {
                return Err(ClaudeFilesystemReadError::UnqualifiedSource);
            }
            let body = split_skill_document(&contents)
                .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?
                .body;
            let description = match metadata.description {
                Some(description) if !description.trim().is_empty() => {
                    description.trim().to_owned()
                }
                _ => native_body_description(body, "Plugin skill")?,
            };
            skills.push(ClaudeFileSkill {
                canonical_name: name,
                description: normalise_claude_description(&description)?,
                display_version: metadata.display_version,
                has_hook_declarations: metadata.has_hook_declarations,
                path,
                scope,
                user_invocable: enabled && metadata.user_invocable.unwrap_or(true),
            });
        }
    }
    skills.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capabilities_plugin_single_root_uses_native_header_name_but_children_use_folder_names()
     {
        let directory = tempfile::tempdir_in("/tmp").expect("fixture");
        let direct = directory.path().join("direct");
        let children = directory.path().join("children");
        fs::create_dir_all(&direct).expect("direct root");
        fs::create_dir_all(children.join("folder-name")).expect("child root");
        fs::write(
            direct.join("SKILL.md"),
            "---\nname: native-plugin:Explicit Name\n---\n# Single root",
        )
        .expect("direct skill");
        fs::write(
            children.join("folder-name/SKILL.md"),
            "---\nname: Display title only\n---\n# Child skill",
        )
        .expect("child skill");
        let skills = read_claude_plugin_skill_roots(
            "2.1.310",
            "native-plugin",
            &[direct, children],
            ClaudeFileSkillScope::User,
            true,
            &CancellationToken::new(),
        )
        .expect("native roots");
        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.canonical_name.as_str())
                .collect::<Vec<_>>(),
            ["native-plugin:Explicit-Name", "native-plugin:folder-name"]
        );
    }
}
