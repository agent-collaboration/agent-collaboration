//! The Router's answer to an auto-mode permission request that stays inside
//! the session's own folders (R4, owner Q3).
//!
//! File reads, edits, deletes and moves whose locations all lie in the working
//! folder or the session's scratch folder are allowed once. A shell command is
//! allowed once only when it runs one of a closed set of file utilities, with no
//! shell syntax, on operands that all lie in the scratch folder. Anything else,
//! or anything the rule cannot read for certain, goes to the approval path.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionId, PermissionOptionKind, ToolCallUpdate, ToolKind,
};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Programs a scratch-only shell command may run without asking: each reads or
/// changes the files it is given and runs no code taken from them.
const FILE_UTILITIES: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "stat", "file", "du", "diff", "cmp", "sort", "uniq", "grep",
    "mkdir", "rmdir", "rm", "cp", "mv", "touch", "ln", "chmod", "find",
];

/// Options a scratch-only shell command may carry: each takes no value.
const VALUELESS_OPTIONS: &[&str] = &[
    "-r", "-R", "-f", "-rf", "-fr", "-Rf", "-fR", "-p", "-l", "-a", "-la", "-al", "-lh", "-h", "-v",
];

/// The folders an auto-mode session acts in without asking, canonical at grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InFolderGrant {
    working_folder: PathBuf,
    scratch_folder: PathBuf,
}

/// Grants by provider session id, for sessions running in auto mode.
pub(crate) type InFolderGrants = Arc<RwLock<HashMap<String, InFolderGrant>>>;

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

    /// A file utility from [`FILE_UTILITIES`], with no shell syntax that could
    /// reach another path or program, whose operands are all paths in the
    /// scratch folder, with at least one.
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
        if !FILE_UTILITIES.contains(&program) {
            return false;
        }
        // Every operand is an absolute path in the scratch folder; a bare word
        // could name a file in the project, so it is not allowed here.
        let mut named_a_path = false;
        for word in words {
            if word.starts_with('-') {
                // An option's meaning is not read from its characters: only
                // known options that take no value pass, so a path glued onto
                // an option (`-ooutside.txt`, `-t/elsewhere`) asks.
                if !VALUELESS_OPTIONS.contains(&word) {
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

/// Canonicalizes the longest existing ancestor and appends the rest, so a file
/// about to be created resolves through any symlinked parent.
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
