//! Claude's native legacy command-to-skill file conversion, without execution.
use super::claude_filesystem::{
    ClaudeFileSkill, ClaudeFilesystemCatalogue, ClaudeFilesystemReadError, ClaudeSkillFileFailure,
    ClaudeSkillOverride, ClaudeSkillRoot,
};
use super::{read_claude_skill_metadata, skill_document::split_skill_document};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub fn read_claude_legacy_skill_files(
    version: &str,
    roots: &[ClaudeSkillRoot],
    overrides: &HashMap<String, ClaudeSkillOverride>,
    cancelled: &CancellationToken,
) -> Result<ClaudeFilesystemCatalogue, ClaudeFilesystemReadError> {
    if version != "2.1.310" {
        return Err(ClaudeFilesystemReadError::UnsupportedVersion);
    }
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    let mut seen_origins = HashSet::new();
    let mut seen_names = HashSet::new();
    for root in roots {
        if !root.directory.is_absolute() {
            return Err(ClaudeFilesystemReadError::InvalidScope);
        }
        let files = scan_command_files(&root.directory, cancelled)?;
        let mut directories = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
        for file in files {
            directories
                .entry(
                    file.parent()
                        .ok_or(ClaudeFilesystemReadError::InvalidScope)?
                        .to_owned(),
                )
                .or_default()
                .push(file);
        }
        for files in directories.into_values() {
            let special = files
                .iter()
                .filter(|file| {
                    file.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("skill.md"))
                })
                .collect::<Vec<_>>();
            if special.len() > 1 {
                return Err(ClaudeFilesystemReadError::AmbiguousOrigin);
            }
            let selected = match special.first() {
                Some(file) => vec![(*file).clone()],
                None => files,
            };
            for path in selected {
                if cancelled.is_cancelled() {
                    return Err(ClaudeFilesystemReadError::Cancelled);
                }
                let real =
                    fs::canonicalize(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
                let name = canonical_legacy_name(&root.directory, &path)?;
                if !seen_origins.insert(real) || !seen_names.insert(name.clone()) {
                    return Err(ClaudeFilesystemReadError::AmbiguousOrigin);
                }
                let candidate = (|| {
                    let file_metadata =
                        fs::metadata(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
                    if !file_metadata.is_file() || file_metadata.len() > 1_048_576 {
                        return Err(ClaudeFilesystemReadError::InvalidMetadata);
                    }
                    let contents = super::skill_file::read_native_skill_file(&path, 1_048_576)
                        .map_err(super::claude_filesystem::map_skill_file_error)?;
                    let metadata = read_claude_skill_metadata(&contents)
                        .map_err(|_| ClaudeFilesystemReadError::InvalidMetadata)?;
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
                        _ => super::claude_filesystem::native_body_description(
                            body,
                            "Custom command",
                        )?,
                    };
                    let description =
                        super::claude_filesystem::normalise_claude_description(&description)?;
                    Ok(ClaudeFileSkill {
                        canonical_name: name.clone(),
                        native_display_name: None,
                        description,
                        display_version: metadata.display_version,
                        has_hook_declarations: metadata.has_hook_declarations,
                        path: path.clone(),
                        scope: root.scope,
                        user_invocable: metadata.user_invocable.unwrap_or(true)
                            && overrides.get(&name) != Some(&ClaudeSkillOverride::Off),
                    })
                })();
                match candidate {
                    Ok(skill) => skills.push(skill),
                    Err(reason) => errors.push(ClaudeSkillFileFailure { path, reason }),
                }
            }
        }
    }
    skills.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    Ok(ClaudeFilesystemCatalogue { skills, errors })
}

fn scan_command_files(
    root: &Path,
    cancelled: &CancellationToken,
) -> Result<Vec<PathBuf>, ClaudeFilesystemReadError> {
    if !root
        .try_exists()
        .map_err(|_| ClaudeFilesystemReadError::Unreadable)?
    {
        return Ok(Vec::new());
    }
    let started = Instant::now();
    let mut stack = vec![root.to_owned()];
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    while let Some(directory) = stack.pop() {
        if cancelled.is_cancelled() {
            return Err(ClaudeFilesystemReadError::Cancelled);
        }
        if started.elapsed() > Duration::from_secs(3) {
            return Err(ClaudeFilesystemReadError::UnqualifiedSource);
        }
        let real =
            fs::canonicalize(&directory).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
        if !seen.insert(real) {
            return Err(ClaudeFilesystemReadError::AmbiguousOrigin);
        }
        for entry in fs::read_dir(&directory).map_err(|_| ClaudeFilesystemReadError::Unreadable)? {
            let entry = entry.map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            let path = entry.path();
            let metadata =
                fs::metadata(&path).map_err(|_| ClaudeFilesystemReadError::Unreadable)?;
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".md"))
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn canonical_legacy_name(root: &Path, path: &Path) -> Result<String, ClaudeFilesystemReadError> {
    let parent = path
        .parent()
        .ok_or(ClaudeFilesystemReadError::InvalidScope)?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| ClaudeFilesystemReadError::InvalidScope)?;
    let mut parts = relative
        .components()
        .map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or(ClaudeFilesystemReadError::InvalidMetadata)?;
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ClaudeFilesystemReadError::InvalidMetadata)?;
    if file.eq_ignore_ascii_case("skill.md") {
        if parts.is_empty() {
            parts.push(
                root.file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(ClaudeFilesystemReadError::InvalidMetadata)?
                    .to_owned(),
            );
        }
    } else {
        parts.push(
            file.strip_suffix(".md")
                .ok_or(ClaudeFilesystemReadError::InvalidMetadata)?
                .to_owned(),
        );
    }
    if parts.iter().any(|part| {
        part.is_empty()
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }) {
        return Err(ClaudeFilesystemReadError::UnqualifiedSource);
    }
    Ok(parts.join(":"))
}

#[cfg(test)]
mod tests {
    use super::super::ClaudeFileSkillScope;
    use super::*;

    #[test]
    fn native_capabilities_legacy_skill_file_suppresses_sibling_commands_and_preserves_namespace() {
        let directory = tempfile::tempdir_in("/tmp").expect("fixture");
        fs::create_dir_all(directory.path().join("group/alpha")).expect("commands");
        fs::write(
            directory.path().join("group/alpha/SKILL.md"),
            "---\nname: Display title\n---\n# Native skill",
        )
        .expect("skill");
        fs::write(directory.path().join("group/alpha/ignored.md"), "# Sibling").expect("sibling");
        fs::write(directory.path().join("group/task.md"), "# Native task").expect("command");
        let roots = [ClaudeSkillRoot {
            directory: directory.path().to_owned(),
            scope: ClaudeFileSkillScope::Workspace,
        }];
        let skills = read_claude_legacy_skill_files(
            "2.1.310",
            &roots,
            &HashMap::new(),
            &CancellationToken::new(),
        )
        .expect("native legacy files")
        .skills;
        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.canonical_name.as_str())
                .collect::<Vec<_>>(),
            ["group:alpha", "group:task"]
        );
        assert_eq!(skills[0].description, "Native skill");
    }
    #[test]
    fn native_capabilities_legacy_bad_candidate_keeps_healthy_file() {
        let directory = tempfile::tempdir_in("/tmp").expect("fixture");
        fs::write(directory.path().join("healthy.md"), "Healthy legacy file").expect("healthy");
        fs::write(
            directory.path().join("broken.md"),
            "---\nname: [broken\n---\n",
        )
        .expect("broken");
        let roots = [ClaudeSkillRoot {
            directory: directory.path().to_owned(),
            scope: ClaudeFileSkillScope::Workspace,
        }];
        let catalogue = read_claude_legacy_skill_files(
            "2.1.310",
            &roots,
            &HashMap::new(),
            &CancellationToken::new(),
        )
        .expect("accounted legacy files");
        assert_eq!(catalogue.skills.len(), 1);
        assert_eq!(catalogue.skills[0].canonical_name, "healthy");
        assert_eq!(
            catalogue.errors,
            vec![ClaudeSkillFileFailure {
                path: directory.path().join("broken.md"),
                reason: ClaudeFilesystemReadError::InvalidMetadata,
            }]
        );
    }
}
