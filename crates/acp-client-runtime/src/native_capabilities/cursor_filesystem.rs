//! Qualified Cursor ACP filesystem skills, using the actual launch workspace.
use super::cursor_skill_metadata::parse_cursor_skill;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

const QUALIFIED_CURSOR_VERSION: &str = "2026.10.22-27dcf5f";
const MAX_NATIVE_SKILL_BYTES: u64 = 1024 * 1024;
const NATIVE_ROOTS: [&str; 5] = [".cursor", ".claude", ".codex", ".grok", ".agents"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorFilesystemContext {
    home: PathBuf,
    workspace: PathBuf,
}

impl CursorFilesystemContext {
    pub fn new(home: PathBuf, workspace: PathBuf) -> Result<Self, CursorFilesystemReadError> {
        if !home.is_absolute()
            || !workspace.is_absolute()
            || home.to_str().is_none()
            || workspace.to_str().is_none()
        {
            return Err(CursorFilesystemReadError::InvalidScope);
        }
        Ok(Self { home, workspace })
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorFileSkillScope {
    User,
    Workspace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorFileSkill {
    pub canonical_name: String,
    pub description: String,
    pub path: PathBuf,
    pub scope: CursorFileSkillScope,
    pub user_invocable: bool,
}

/// An accounted directory can contain both usable files and located failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorFilesystemCatalogue {
    pub skills: Vec<CursorFileSkill>,
    pub errors: Vec<CursorSkillFileFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorSkillFileFailure {
    pub path: PathBuf,
    pub reason: CursorSkillFileFailureReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorSkillFileFailureReason {
    Unreadable,
    InvalidMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CursorFilesystemReadError {
    #[error("Cursor filesystem source version is unqualified")]
    UnsupportedVersion,
    #[error("Cursor filesystem scope does not match its launch workspace")]
    InvalidScope,
    #[error("Cursor native skill source could not be read")]
    Unreadable { path: PathBuf },
    #[error("Cursor native skill metadata is invalid or unrepresentable")]
    InvalidMetadata { path: PathBuf },
    #[error("Cursor native skill source aliases are ambiguous")]
    AmbiguousOrigin { path: PathBuf },
    #[error("Cursor filesystem read cancelled")]
    Cancelled,
}

#[derive(Clone, Copy)]
enum RootScope {
    Builtin,
    Workspace,
    User,
}

struct SkillCandidate {
    path: PathBuf,
    real_path: PathBuf,
}

/// Blocking file I/O; the existing runtime calls this from its read worker.
pub fn read_cursor_filesystem_skills(
    context: &CursorFilesystemContext,
    runtime_version: &str,
    requested_workspace: &Path,
    cancelled: &CancellationToken,
) -> Result<CursorFilesystemCatalogue, CursorFilesystemReadError> {
    if runtime_version != QUALIFIED_CURSOR_VERSION {
        return Err(CursorFilesystemReadError::UnsupportedVersion);
    }
    let requested = fs::canonicalize(requested_workspace).map_err(|_| {
        tracing::warn!(
            phase = "cursor-filesystem-scope",
            category = "requested-workspace-unresolved",
            "native catalogue scope qualification failed"
        );
        CursorFilesystemReadError::InvalidScope
    })?;
    let launched = fs::canonicalize(&context.workspace).map_err(|_| {
        tracing::warn!(
            phase = "cursor-filesystem-scope",
            category = "launch-workspace-unresolved",
            "native catalogue scope qualification failed"
        );
        CursorFilesystemReadError::InvalidScope
    })?;
    tracing::info!(
        phase = "cursor-filesystem-scope",
        category = "canonical-workspace-relation",
        provider = "cursor",
        requested_equals_launch_workspace = requested_workspace == context.workspace(),
        canonical_workspaces_equal = requested == launched,
        "native catalogue workspace relation checked"
    );
    if requested != launched {
        tracing::warn!(
            phase = "cursor-filesystem-scope",
            category = "requested-workspace-mismatch",
            "native catalogue scope qualification failed"
        );
        return Err(CursorFilesystemReadError::InvalidScope);
    }
    let discovery_workspace = native_cursor_discovery_root(&launched)?;
    // This order is native CustomCommandsLoader.loadSkillRoots: builtin,
    // workspace roots, then user roots. The native user map overwrites names.
    let mut roots = vec![(
        context.home.join(".cursor/skills-cursor"),
        RootScope::Builtin,
    )];
    roots.extend(NATIVE_ROOTS.map(|config| {
        (
            discovery_workspace.join(config).join("skills"),
            RootScope::Workspace,
        )
    }));
    roots.extend(
        NATIVE_ROOTS.map(|config| (context.home.join(config).join("skills"), RootScope::User)),
    );
    let mut seen_files = HashSet::new();
    let mut active = BTreeMap::<String, (RootScope, CursorFileSkill)>::new();
    let mut disabled = Vec::new();
    let mut errors = Vec::new();
    for (root, scope) in roots {
        check_cancelled(cancelled)?;
        let candidates = collect_skill_files(&root, matches!(scope, RootScope::User), cancelled)?;
        let mut distinct = Vec::new();
        for candidate in candidates {
            if excluded_native_skill(&candidate.path) {
                continue;
            }
            if seen_files.insert(candidate.real_path.clone()) {
                distinct.push(candidate);
            }
        }
        let mut folder_counts = HashMap::<String, usize>::new();
        for candidate in &distinct {
            *folder_counts
                .entry(folder_name(&candidate.path)?)
                .or_default() += 1;
        }
        let mut root_names = HashSet::new();
        for candidate in distinct {
            check_cancelled(cancelled)?;
            let folder = folder_name(&candidate.path)?;
            let canonical_name = if folder_counts.get(&folder).copied().unwrap_or(0) > 1 {
                relative_skill_name(&root, &candidate.path)?
            } else {
                folder
            };
            if !root_names.insert(canonical_name.clone()) {
                // Native appends order-dependent numeric suffixes here. An
                // ambiguous origin is not guessed into a selectable identity.
                return Err(CursorFilesystemReadError::AmbiguousOrigin {
                    path: candidate.path,
                });
            }
            // Bundled files are outside this view. Their real paths have
            // already participated in native alias deduplication above.
            if matches!(scope, RootScope::Builtin) {
                continue;
            }
            let metadata = match read_candidate_metadata(&candidate.path, &canonical_name) {
                Ok(Some(metadata)) => metadata,
                Ok(None) => continue,
                Err(reason) => {
                    errors.push(CursorSkillFileFailure {
                        path: candidate.path,
                        reason,
                    });
                    continue;
                }
            };
            let skill = CursorFileSkill {
                canonical_name: canonical_name.clone(),
                description: metadata.description,
                path: candidate.path,
                scope: match scope {
                    RootScope::User | RootScope::Builtin => CursorFileSkillScope::User,
                    RootScope::Workspace => CursorFileSkillScope::Workspace,
                },
                user_invocable: metadata.user_invocable,
            };
            if skill.user_invocable {
                active.insert(canonical_name, (scope, skill));
            } else if !matches!(scope, RootScope::Builtin) {
                // FilesystemCatalogues retains the observed disabled file.
                // NativeLoaded does not insert it into the active-name map.
                disabled.push(skill);
            }
        }
    }
    check_cancelled(cancelled)?;
    let mut skills = active
        .into_values()
        .filter_map(|(scope, skill)| (!matches!(scope, RootScope::Builtin)).then_some(skill))
        .collect::<Vec<_>>();
    skills.extend(disabled);
    Ok(CursorFilesystemCatalogue { skills, errors })
}

fn read_candidate_metadata(
    path: &Path,
    canonical_name: &str,
) -> Result<Option<super::cursor_skill_metadata::CursorSkillMetadata>, CursorSkillFileFailureReason>
{
    let text = super::skill_file::read_native_skill_file(path, MAX_NATIVE_SKILL_BYTES).map_err(
        |error| match error {
            super::skill_file::NativeSkillFileError::Unreadable => {
                CursorSkillFileFailureReason::Unreadable
            }
            super::skill_file::NativeSkillFileError::InvalidMetadata => {
                CursorSkillFileFailureReason::InvalidMetadata
            }
        },
    )?;
    parse_cursor_skill(&text, canonical_name)
        .map_err(|_| CursorSkillFileFailureReason::InvalidMetadata)
}

fn native_cursor_discovery_root(workspace: &Path) -> Result<PathBuf, CursorFilesystemReadError> {
    // ACP Session.getProjectRoot calls utils/git.ts ky/f: nearest existing
    // .git marker, with the original process cwd as its non-repository fallback.
    let mut directory = workspace;
    loop {
        if directory
            .join(".git")
            .try_exists()
            .map_err(|_| unreadable(directory))?
        {
            return Ok(directory.to_owned());
        }
        let Some(parent) = directory.parent() else {
            break;
        };
        if parent == directory || parent.parent().is_none() {
            break;
        }
        directory = parent;
    }
    Ok(workspace.to_owned())
}

fn collect_skill_files(
    root: &Path,
    allow_user_symlink: bool,
    cancelled: &CancellationToken,
) -> Result<Vec<SkillCandidate>, CursorFilesystemReadError> {
    let real_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unreadable(root)),
    };
    let mut stack = vec![(root.to_owned(), 0usize, false)];
    let mut seen_directories = HashMap::<PathBuf, PathBuf>::new();
    let mut seen_root_files = HashSet::new();
    let mut files = Vec::new();
    while let Some((directory, depth, escaped)) = stack.pop() {
        check_cancelled(cancelled)?;
        if depth > 10 {
            continue;
        }
        let real_directory = fs::canonicalize(&directory).map_err(|_| unreadable(&directory))?;
        if !escaped && !real_directory.starts_with(&real_root) {
            continue;
        }
        if let Some(previous) = seen_directories.insert(real_directory, directory.clone()) {
            if previous != directory {
                return Err(CursorFilesystemReadError::AmbiguousOrigin { path: directory });
            }
            continue;
        }
        let entries = fs::read_dir(&directory).map_err(|_| unreadable(&directory))?;
        for entry in entries {
            let entry = entry.map_err(|_| unreadable(&directory))?;
            let path = entry.path();
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| CursorFilesystemReadError::InvalidMetadata { path: path.clone() })?;
            let file_type = entry.file_type().map_err(|_| unreadable(&path))?;
            let is_symlink = file_type.is_symlink();
            let metadata = fs::metadata(&path).map_err(|_| unreadable(&path))?;
            if metadata.is_dir() {
                if !escaped && !ignored_directory(&name) {
                    stack.push((path, depth + 1, allow_user_symlink && is_symlink));
                }
            } else if name == "SKILL.md" {
                let real_path = fs::canonicalize(&path).map_err(|_| unreadable(&path))?;
                if (!escaped || is_symlink) && !real_path.starts_with(&real_root) {
                    continue;
                }
                if !seen_root_files.insert(real_path.clone()) {
                    return Err(CursorFilesystemReadError::AmbiguousOrigin { path });
                }
                files.push(SkillCandidate { path, real_path });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn folder_name(path: &Path) -> Result<String, CursorFilesystemReadError> {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CursorFilesystemReadError::InvalidMetadata {
            path: path.to_owned(),
        })
}

fn relative_skill_name(root: &Path, path: &Path) -> Result<String, CursorFilesystemReadError> {
    let relative = path
        .parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
        .ok_or_else(|| CursorFilesystemReadError::InvalidMetadata {
            path: path.to_owned(),
        })?;
    let names = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CursorFilesystemReadError::InvalidMetadata {
            path: path.to_owned(),
        })?;
    Ok(names.join("-"))
}

fn ignored_directory(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "__pycache__" | "dist" | "build")
}

fn excluded_native_skill(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let codex_root = components
        .windows(2)
        .any(|pair| pair == [".codex", "skills"]);
    codex_root
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                matches!(
                    name,
                    "imagegen"
                        | "openai-docs"
                        | "opneai-docs"
                        | "plugin-creator"
                        | "skill-creator"
                        | "skill-installer"
                )
            })
}

fn unreadable(path: &Path) -> CursorFilesystemReadError {
    CursorFilesystemReadError::Unreadable {
        path: path.to_owned(),
    }
}

fn check_cancelled(cancelled: &CancellationToken) -> Result<(), CursorFilesystemReadError> {
    if cancelled.is_cancelled() {
        Err(CursorFilesystemReadError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "cursor_filesystem_tests.rs"]
mod tests;
