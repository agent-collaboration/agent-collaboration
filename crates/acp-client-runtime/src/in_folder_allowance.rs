//! The Router's answer to an auto-mode permission request that stays inside
//! the session's own folders (R4, owner Q3).
//!
//! File reads, edits, deletes and moves whose named paths all lie in the working
//! folder, the session's scratch folder or one of the Router's work roots
//! (D1 R18: same rule, wider set) are allowed once. A request names its
//! paths in ACP `locations`, in `diff` content, or, for an agent that states
//! them only in the title (Cursor), as its provider behaviour reads that title. A shell command is
//! allowed once only when it runs one of a closed set of file utilities, with no
//! shell syntax, on operands that all lie in the scratch folder or a work root. Anything else,
//! or anything the rule cannot read for certain, goes to the approval path.
//!
//! The rule decides only the requests an agent sends. Claude in acceptEdits runs
//! its own auto-approved file commands (mkdir, touch, rm, rmdir, mv, cp, sed) in
//! its working folders, the scratch folder included, without asking.

use crate::{PermissionRequestAction, StatedPermissionTarget};
use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionId, PermissionOptionKind, ToolCallContent, ToolCallUpdate,
    ToolKind,
};

use std::path::{Component, Path, PathBuf};

/// Programs a scratch-only shell command may run without asking, each with the
/// options it may carry. Every program reads or changes only the files it is
/// given and runs no code taken from them; every option takes no value and
/// reads nothing indirectly (checked against the BSD and GNU manuals). A
/// program with no options listed takes none. `cp` is absent: copying into a
/// directory writes a name it derives, which can be a symlink out of the folder.
/// `mv` replaces that entry and `ln` refuses an existing one, so they stay.
/// `diff` is absent: comparing directories reads children by name and follows
/// a child link out. `chmod` is absent: its mode is a bare word, never allowed.
/// `sort` is absent: input larger than its memory buffer spills to temporary
/// files in the system `$TMPDIR` (or `/var/tmp`), outside the folder.
const FILE_UTILITIES: &[(&str, &[&str])] = &[
    ("rm", &["-r", "-R", "-f", "-rf", "-fr", "-Rf", "-fR", "-v"]),
    ("rmdir", &["-v"]),
    ("mkdir", &["-p", "-v"]),
    ("mv", &["-v"]),
    ("ls", &["-l", "-a", "-la", "-al", "-lh", "-h"]),
    ("ln", &["-s"]),
    ("cat", &[]),
    ("head", &[]),
    ("tail", &[]),
    ("wc", &[]),
    ("stat", &[]),
    ("file", &[]),
    ("du", &[]),
    ("cmp", &[]),
    ("uniq", &[]),
    ("grep", &[]),
    ("touch", &[]),
    ("find", &[]),
];

/// Reads the target an agent states in a request's title: the provider
/// behaviour's `stated_permission_target`.
pub(crate) type StatedTargetReader<'a> =
    dyn Fn(PermissionRequestAction, &str) -> Option<StatedPermissionTarget> + 'a;

/// The folders an auto-mode session acts in without asking, canonical at grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InFolderGrant {
    working_folder: PathBuf,
    scratch_folder: PathBuf,
    /// The Router's work roots that exist (D1 R18).
    work_roots: Vec<PathBuf>,
}

/// A session's folders beyond its working folder, as an agent is told them:
/// the scratch folder, then the work roots that exist, once each.
pub(crate) fn session_folders(
    scratch_folder: Option<&Path>,
    work_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = scratch_folder.map(Path::to_path_buf).into_iter().collect();
    for root in work_roots {
        if root.is_dir() && !folders.contains(root) {
            folders.push(root.clone());
        }
    }
    folders
}

impl InFolderGrant {
    /// `None` when the working or scratch folder cannot be resolved; such a
    /// session asks for everything rather than allowing against a path it
    /// cannot pin down. A work root that doesn't exist on this computer is
    /// left out.
    pub(crate) fn new(
        working_folder: &Path,
        scratch_folder: &Path,
        work_roots: &[PathBuf],
    ) -> Option<Self> {
        Some(Self {
            working_folder: std::fs::canonicalize(working_folder).ok()?,
            scratch_folder: std::fs::canonicalize(scratch_folder).ok()?,
            work_roots: work_roots
                .iter()
                .filter_map(|root| std::fs::canonicalize(root).ok())
                .collect(),
        })
    }

    /// Check the normalized request against this run's folders without I/O.
    pub(crate) fn select(&self, permission: InFolderPermission) -> Option<PermissionOptionId> {
        permission
            .paths
            .iter()
            .all(|path| {
                (permission.scope == PermissionPathScope::File
                    && path.starts_with(&self.working_folder))
                    || path.starts_with(&self.scratch_folder)
                    || self.work_roots.iter().any(|root| path.starts_with(root))
            })
            .then_some(permission.option)
    }

    #[cfg(test)]
    fn allow_once(
        &self,
        tool_call: &ToolCallUpdate,
        options: &[PermissionOption],
        read_title: &StatedTargetReader<'_>,
    ) -> Option<PermissionOptionId> {
        self.select(InFolderPermission::from_offer(
            tool_call, options, read_title,
        )?)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionPathScope {
    File,
    Command,
}

/// A sole allow-once option and the request's canonical paths, resolved before
/// the transport locks the route to make its permission decision.
pub(crate) struct InFolderPermission {
    option: PermissionOptionId,
    paths: Vec<PathBuf>,
    scope: PermissionPathScope,
}

impl InFolderPermission {
    pub(crate) fn from_offer(
        tool_call: &ToolCallUpdate,
        options: &[PermissionOption],
        read_title: &StatedTargetReader<'_>,
    ) -> Option<Self> {
        let mut allow_once = options
            .iter()
            .filter(|option| option.kind == PermissionOptionKind::AllowOnce);
        let option = allow_once.next()?.option_id.clone();
        if allow_once.next().is_some() {
            return None;
        }
        let fields = &tool_call.fields;
        let stated = |action| {
            fields
                .title
                .as_deref()
                .and_then(|title| read_title(action, title))
        };
        let (paths, scope) = match fields.kind {
            Some(ToolKind::Read | ToolKind::Edit | ToolKind::Delete | ToolKind::Move) => {
                let stated_file = match stated(PermissionRequestAction::FileAccess) {
                    Some(StatedPermissionTarget::File(path)) => Some(path),
                    Some(StatedPermissionTarget::Command(_)) => return None,
                    None => None,
                };
                let located = fields
                    .locations
                    .iter()
                    .flatten()
                    .map(|location| location.path.as_path());
                let diffed = fields
                    .content
                    .iter()
                    .flatten()
                    .filter_map(|content| match content {
                        ToolCallContent::Diff(diff) => Some(diff.path.as_path()),
                        _ => None,
                    });
                let paths: Vec<PathBuf> = located
                    .chain(diffed)
                    .chain(stated_file.as_deref())
                    .map(permission_path)
                    .collect::<Option<_>>()?;
                (paths, PermissionPathScope::File)
            }
            Some(ToolKind::Execute) => {
                let stated_command = match stated(PermissionRequestAction::ShellCommand) {
                    Some(StatedPermissionTarget::Command(command)) => Some(command),
                    Some(StatedPermissionTarget::File(_)) => return None,
                    None => None,
                };
                let raw_command = fields
                    .raw_input
                    .as_ref()
                    .and_then(|input| input.get("command"))
                    .and_then(serde_json::Value::as_str);
                let mut paths = Vec::new();
                for command in raw_command.into_iter().chain(stated_command.as_deref()) {
                    paths.extend(command_paths(command)?);
                }
                (paths, PermissionPathScope::Command)
            }
            _ => return None,
        };
        (!paths.is_empty()).then_some(Self {
            option,
            paths,
            scope,
        })
    }
}

/// A named file utility with its own options and at least one canonical path.
fn command_paths(command: &str) -> Option<Vec<PathBuf>> {
    const SHELL_SYNTAX: &[char] = &[
        '$', '`', '|', ';', '&', '<', '>', '(', ')', '{', '}', '*', '?', '[', ']', '~', '\\', '"',
        '\'', '\n', '\r', '#', '!', '=',
    ];
    if command.contains(SHELL_SYNTAX) {
        return None;
    }
    let mut words = command.split_whitespace();
    let program = words.next()?;
    let (_, options) = FILE_UTILITIES
        .iter()
        .find(|(utility, _)| *utility == program)?;
    let mut paths = Vec::new();
    for word in words {
        if word.starts_with('-') {
            if !options.contains(&word) {
                return None;
            }
        } else {
            paths.push(permission_path(Path::new(word))?);
        }
    }
    (!paths.is_empty()).then_some(paths)
}

fn permission_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    canonical_existing_prefix(path)
}

/// Canonicalizes the longest existing ancestor and appends the absent rest, so
/// a file about to be created resolves through any symlinked parent. `None` when
/// a present component cannot be resolved, such as a dangling symlink.
fn canonical_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match std::fs::canonicalize(existing) {
            Ok(mut resolved) => {
                for part in missing.iter().rev() {
                    resolved.push(part);
                }
                return Some(resolved);
            }
            // Only a name that is genuinely absent may be appended unresolved.
            // Something present that still cannot be resolved (a dangling or
            // unreadable symlink) would let a write follow it out of the folder.
            Err(_) if std::fs::symlink_metadata(existing).is_ok() => return None,
            Err(_) => {
                missing.push(existing.file_name()?.to_owned());
                existing = existing.parent()?;
            }
        }
    }
}

#[cfg(test)]
#[path = "in_folder_allowance_test.rs"]
mod tests;
