//! The Router's answer to an auto-mode permission request that stays inside
//! the session's own folders (R4, owner Q3).
//!
//! File reads, edits, deletes and moves whose locations all lie in the working
//! folder or the session's scratch folder are allowed once. A shell command is
//! allowed once only when it runs one of a closed set of file utilities, with no
//! shell syntax, on operands that all lie in the scratch folder. Anything else,
//! or anything the rule cannot read for certain, goes to the approval path.
//!
//! The rule decides only the requests an agent sends. Claude in acceptEdits runs
//! its own auto-approved file commands (mkdir, touch, rm, rmdir, mv, cp, sed) in
//! its working folders, the scratch folder included, without asking.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionId, PermissionOptionKind, ToolCallUpdate, ToolKind,
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
    ("sort", &[]),
    ("uniq", &[]),
    ("grep", &[]),
    ("touch", &[]),
    ("find", &[]),
];

/// The folders an auto-mode session acts in without asking, canonical at grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InFolderGrant {
    working_folder: PathBuf,
    scratch_folder: PathBuf,
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
    /// `None` when either folder cannot be resolved; such a session asks for
    /// everything rather than allowing against a path it cannot pin down.
    pub(crate) fn new(working_folder: &Path, scratch_folder: &Path) -> Option<Self> {
        Some(Self {
            working_folder: std::fs::canonicalize(working_folder).ok()?,
            scratch_folder: std::fs::canonicalize(scratch_folder).ok()?,
        })
    }

    /// The agent's allow-once option when the request lies within the grant.
    pub(crate) fn allow_once(
        &self,
        tool_call: &ToolCallUpdate,
        options: &[PermissionOption],
    ) -> Option<PermissionOptionId> {
        if !self.covers(tool_call) {
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

    fn covers(&self, tool_call: &ToolCallUpdate) -> bool {
        let fields = &tool_call.fields;
        match fields.kind {
            Some(ToolKind::Read | ToolKind::Edit | ToolKind::Delete | ToolKind::Move) => {
                match fields.locations.as_deref() {
                    Some(locations) if !locations.is_empty() => locations.iter().all(|location| {
                        within(&location.path, &self.working_folder)
                            || within(&location.path, &self.scratch_folder)
                    }),
                    _ => false,
                }
            }
            Some(ToolKind::Execute) => fields
                .raw_input
                .as_ref()
                .and_then(|input| input.get("command"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|command| self.command_stays_in_scratch(command)),
            _ => false,
        }
    }

    /// A file utility from [`FILE_UTILITIES`] with only its own options, no
    /// shell syntax that could reach another path or program, and operands
    /// that are all paths in the scratch folder, with at least one.
    fn command_stays_in_scratch(&self, command: &str) -> bool {
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
        // Every operand is an absolute path in the scratch folder; a bare word
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
            if !within(Path::new(word), &self.scratch_folder) {
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
