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
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

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

/// A session's in-folder roots, and whether Router auto is its current mode.
/// The roots outlive a change to a named mode, so a later change back to auto
/// restores the same bounded grant.
#[derive(Clone, Debug)]
pub(crate) struct SessionInFolderGrant {
    pub(crate) grant: InFolderGrant,
    pub(crate) router_auto: bool,
}

/// In-folder grants by provider session id.
pub(crate) type InFolderGrants = Arc<RwLock<HashMap<String, SessionInFolderGrant>>>;

/// The grant a permission request may use: only while the session runs in
/// Router auto.
pub(crate) fn active_grant(grants: &InFolderGrants, session_id: &str) -> Option<InFolderGrant> {
    grants
        .read()
        .ok()
        .and_then(|grants| grants.get(session_id).cloned())
        .filter(|session| session.router_auto)
        .map(|session| session.grant)
}

/// Records the session's mode after a confirmed mode change (R4, Q3).
pub(crate) fn set_router_auto(grants: &InFolderGrants, session_id: &str, router_auto: bool) {
    if let Ok(mut grants) = grants.write()
        && let Some(session) = grants.get_mut(session_id)
    {
        session.router_auto = router_auto;
    }
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

    /// Whether a path lies in one of the folders a shell operand may name:
    /// the scratch folder or a work root.
    fn in_command_folders(&self, path: &Path) -> bool {
        within(path, &self.scratch_folder) || self.work_roots.iter().any(|root| within(path, root))
    }

    /// The agent's allow-once option when the request lies within the grant.
    pub(crate) fn allow_once(
        &self,
        tool_call: &ToolCallUpdate,
        options: &[PermissionOption],
        read_title: &StatedTargetReader<'_>,
    ) -> Option<PermissionOptionId> {
        if !self.covers(tool_call, read_title) {
            return None;
        }
        let mut allow_once = options
            .iter()
            .filter(|option| option.kind == PermissionOptionKind::AllowOnce);
        let option = allow_once.next()?;
        allow_once
            .next()
            .is_none()
            .then(|| option.option_id.clone())
    }

    /// Every path or command the request names, from any of its fields, must
    /// stay inside, and it must name at least one.
    fn covers(&self, tool_call: &ToolCallUpdate, read_title: &StatedTargetReader<'_>) -> bool {
        let fields = &tool_call.fields;
        let stated = |action| {
            fields
                .title
                .as_deref()
                .and_then(|title| read_title(action, title))
        };
        match fields.kind {
            Some(ToolKind::Read | ToolKind::Edit | ToolKind::Delete | ToolKind::Move) => {
                let stated_file = match stated(PermissionRequestAction::FileAccess) {
                    Some(StatedPermissionTarget::File(path)) => Some(path),
                    Some(StatedPermissionTarget::Command(_)) => return false,
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
                let paths: Vec<&Path> = located
                    .chain(diffed)
                    .chain(stated_file.as_deref())
                    .collect();
                !paths.is_empty()
                    && paths.iter().all(|path| {
                        within(path, &self.working_folder) || self.in_command_folders(path)
                    })
            }
            Some(ToolKind::Execute) => {
                let stated_command = match stated(PermissionRequestAction::ShellCommand) {
                    Some(StatedPermissionTarget::Command(command)) => Some(command),
                    Some(StatedPermissionTarget::File(_)) => return false,
                    None => None,
                };
                let raw_command = fields
                    .raw_input
                    .as_ref()
                    .and_then(|input| input.get("command"))
                    .and_then(serde_json::Value::as_str);
                let commands: Vec<&str> = raw_command
                    .into_iter()
                    .chain(stated_command.as_deref())
                    .collect();
                !commands.is_empty()
                    && commands
                        .iter()
                        .all(|command| self.command_stays_inside(command))
            }
            _ => false,
        }
    }

    /// A file utility from [`FILE_UTILITIES`] with only its own options, no
    /// shell syntax that could reach another path or program, and operands
    /// that are all paths in the scratch folder or a work root, with at least
    /// one.
    fn command_stays_inside(&self, command: &str) -> bool {
        const SHELL_SYNTAX: &[char] = &[
            '$', '`', '|', ';', '&', '<', '>', '(', ')', '{', '}', '*', '?', '[', ']', '~', '\\',
            '"', '\'', '\n', '\r', '#', '!', '=',
        ];
        if command.contains(SHELL_SYNTAX) {
            return false;
        }
        let mut words = command.split_whitespace();
        let Some(program) = words.next() else {
            return false;
        };
        // Only file utilities that run no code given to them: an interpreter,
        // `env` or `xargs` would run a script the agent first wrote to scratch,
        // and that script could act anywhere (owner Q3).
        let Some((_, options)) = FILE_UTILITIES
            .iter()
            .find(|(utility, _)| *utility == program)
        else {
            return false;
        };
        // Every operand is an absolute path in the scratch folder or a work
        // root; a bare word
        // could name a file in the project, so it is not allowed here.
        let mut named_a_path = false;
        for word in words {
            if word.starts_with('-') {
                // An option's meaning is not read from its characters: only
                // this utility's listed options pass, so a path glued onto an
                // option (`-ooutside.txt`) or an indirect one (`file -f`) asks.
                if !options.contains(&word) {
                    return false;
                }
                continue;
            }
            if !self.in_command_folders(Path::new(word)) {
                return false;
            }
            named_a_path = true;
        }
        named_a_path
    }
}

/// Whether an absolute path, with no `..`, resolves inside `folder` once its
/// existing part is canonical (so a symlink out of the folder is outside).
fn within(path: &Path, folder: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    canonical_existing_prefix(path).is_some_and(|resolved| resolved.starts_with(folder))
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
