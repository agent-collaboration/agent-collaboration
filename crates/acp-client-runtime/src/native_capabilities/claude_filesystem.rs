//! Read qualified ordinary Claude skill roots; execution and expansion stay native.
use super::{
    claude_skill_frontmatter::read_claude_skill_metadata, skill_document::split_skill_document,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaudeFileSkillScope {
    User,
    Workspace,
    Managed,
}

#[derive(Clone, Debug)]
pub struct ClaudeSkillRoot {
    pub directory: PathBuf,
    pub scope: ClaudeFileSkillScope,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ClaudeSkillOverride {
    On,
    NameOnly,
    UserInvocableOnly,
    Off,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeFileSkill {
    pub canonical_name: String,
    pub description: String,
    pub display_version: Option<String>,
    pub has_hook_declarations: bool,
    pub path: PathBuf,
    pub scope: ClaudeFileSkillScope,
    pub user_invocable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeFilesystemCatalogue {
    pub skills: Vec<ClaudeFileSkill>,
    pub errors: Vec<ClaudeSkillFileFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeSkillFileFailure {
    pub path: PathBuf,
    pub reason: ClaudeFilesystemReadError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClaudeFilesystemReadError {
    #[error("Claude native skill source version is unqualified")]
    UnsupportedVersion,
    #[error("Claude native skill root is unqualified")]
    InvalidScope,
    #[error("Claude native skill source is unreadable")]
    Unreadable,
    #[error("Claude native skill metadata is invalid")]
    InvalidMetadata,
    #[error("Claude native skill source needs additional qualification")]
    UnqualifiedSource,
    #[error("Claude native skill identity or origin is ambiguous")]
    AmbiguousOrigin,
    #[error("Claude native skill read cancelled")]
    Cancelled,
}

/// Roots and effective ordinary-name overrides must come from the binding's
/// qualified setup reader. This function does not infer roots from a request.
pub fn read_claude_filesystem_skills(
    runtime_version: &str,
    roots: &[ClaudeSkillRoot],
    overrides: &HashMap<String, ClaudeSkillOverride>,
    cancelled: &CancellationToken,
) -> Result<ClaudeFilesystemCatalogue, ClaudeFilesystemReadError> {
    if runtime_version != "2.1.310" {
        return Err(ClaudeFilesystemReadError::UnsupportedVersion);
    }
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_origins = HashSet::new();
    for root in roots {
        check_cancelled(cancelled)?;
        if !root.directory.is_absolute() {
            return Err(ClaudeFilesystemReadError::InvalidScope);
        }
        let entries = match fs::read_dir(&root.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(ClaudeFilesystemReadError::Unreadable),
        };
        for entry in entries {
            check_cancelled(cancelled)?;
            let entry = entry.map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            let metadata = entry
                .metadata()
                .map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            if !metadata.is_dir()
                && !entry
                    .file_type()
                    .map_err(|_| ClaudeFilesystemReadError::Unreadable)?
                    .is_symlink()
            {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?;
            if name.eq_ignore_ascii_case("synced") {
                // Native AL excludes this reserved root from the ordinary
                // folder view. The setup reader must qualify synced separately.
                continue;
            }
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(ClaudeFilesystemReadError::UnqualifiedSource);
            }
            let directory = entry.path();
            if directory
                .join(".claude-plugin")
                .try_exists()
                .map_err(|_| ClaudeFilesystemReadError::Unreadable)?
            {
                return Err(ClaudeFilesystemReadError::UnqualifiedSource);
            }
            let path = directory.join("SKILL.md");
            let file_metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(ClaudeFilesystemReadError::Unreadable),
            };
            let real_path =
                fs::canonicalize(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            if !seen_origins.insert(real_path) || !seen_names.insert(name.clone()) {
                return Err(ClaudeFilesystemReadError::AmbiguousOrigin);
            }
            let candidate = (|| {
                if !file_metadata.is_file() || file_metadata.len() > 1_000_000 {
                    return Err(ClaudeFilesystemReadError::InvalidMetadata);
                }
                let contents = super::skill_file::read_native_skill_file(&path, 1_000_000)
                    .map_err(map_skill_file_error)?;
                let metadata = read_claude_skill_metadata(&contents)
                    .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?;
                if metadata.has_conditional_paths || metadata.fallback == Some(true) {
                    return Err(ClaudeFilesystemReadError::UnqualifiedSource);
                }
                let document = split_skill_document(&contents)
                    .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?;
                let description = match metadata.description {
                    Some(description) if !description.trim().is_empty() => {
                        description.trim().to_owned()
                    }
                    _ => native_body_description(document.body, "Skill")?,
                };
                let description = normalise_claude_description(&description)?;
                let enabled_by_setting = overrides.get(&name) != Some(&ClaudeSkillOverride::Off);
                Ok(ClaudeFileSkill {
                    canonical_name: name,
                    description,
                    display_version: metadata.display_version,
                    has_hook_declarations: metadata.has_hook_declarations,
                    path: path.clone(),
                    scope: root.scope,
                    user_invocable: enabled_by_setting && metadata.user_invocable.unwrap_or(true),
                })
            })();
            match candidate {
                Ok(skill) => skills.push(skill),
                Err(reason) => errors.push(ClaudeSkillFileFailure { path, reason }),
            }
        }
    }
    check_cancelled(cancelled)?;
    skills.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    Ok(ClaudeFilesystemCatalogue { skills, errors })
}

pub(super) fn native_body_description(
    body: &str,
    fallback: &str,
) -> Result<String, ClaudeFilesystemReadError> {
    let Some(line) = body.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return Ok(fallback.to_owned());
    };
    let without_hashes = line.trim_start_matches('#');
    let description =
        if without_hashes.len() != line.len() && without_hashes.starts_with(char::is_whitespace) {
            without_hashes.trim_start()
        } else {
            line
        };
    // Native slices UTF-16 units. A split surrogate is not representable in
    // Rust UTF-8 without changing its value, so report it explicitly.
    let encoded = description.encode_utf16().collect::<Vec<_>>();
    if encoded.len() > 100 {
        let mut value = String::from_utf16(encoded.get(..97).unwrap_or_default())
            .map_err(|_| ClaudeFilesystemReadError::UnqualifiedSource)?;
        value.push_str("...");
        Ok(value)
    } else {
        Ok(description.to_owned())
    }
}

/// Native Fbt/dt normalizes display text after frontmatter decoding. Preserve
/// only the character classes qualified here; unsupported formatting errors
/// rather than being reported as the effective native description unchanged.
pub(super) fn normalise_claude_description(
    value: &str,
) -> Result<String, ClaudeFilesystemReadError> {
    let mut normalized = String::new();
    let mut replacing_controls = false;
    for character in value.chars() {
        if character == '\u{1b}' {
            return Err(ClaudeFilesystemReadError::UnqualifiedSource);
        }
        if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
            if !replacing_controls {
                normalized.push(' ');
            }
            replacing_controls = true;
            continue;
        }
        replacing_controls = false;
        if matches!(character, '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}') {
            return Err(ClaudeFilesystemReadError::UnqualifiedSource);
        }
        if !character.is_ascii()
            && !character.is_alphanumeric()
            && !matches!(
                character,
                '—' | '–'
                    | '…'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '。'
                    | '，'
                    | '：'
                    | '！'
                    | '？'
                    | '、'
            )
        {
            return Err(ClaudeFilesystemReadError::UnqualifiedSource);
        }
        match character {
            '<' => normalized.push_str("&lt;"),
            '>' => normalized.push_str("&gt;"),
            _ => normalized.push(character),
        }
    }
    Ok(normalized)
}

pub(super) fn map_skill_file_error(
    error: super::skill_file::NativeSkillFileError,
) -> ClaudeFilesystemReadError {
    match error {
        super::skill_file::NativeSkillFileError::Unreadable => {
            ClaudeFilesystemReadError::Unreadable
        }
        super::skill_file::NativeSkillFileError::InvalidMetadata => {
            ClaudeFilesystemReadError::InvalidMetadata
        }
    }
}

fn check_cancelled(cancelled: &CancellationToken) -> Result<(), ClaudeFilesystemReadError> {
    if cancelled.is_cancelled() {
        Err(ClaudeFilesystemReadError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "claude_filesystem_tests.rs"]
mod tests;
