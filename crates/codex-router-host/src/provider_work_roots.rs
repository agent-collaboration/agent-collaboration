//! The work roots a Router-started session in auto works in without asking
//! (D1 R18). One list from settings.jsonc, plus the session's project tmp/
//! and the roots its creator named; the session's scratch folder joins them in
//! the runtime. Claude gets them as additional directories and a PreToolUse
//! hook that asks outside them; Cursor gets them through the Router's
//! in-folder allowance and ACP additional directories.

use std::fs;
use std::io;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

/// The hook the Router installs for Claude sessions it starts in auto.
const WORK_ROOTS_HOOK_SOURCE: &str = include_str!("provider_work_roots/router-work-roots-hook.ts");
const WORK_ROOTS_HOOK_FILE: &str = "router-work-roots-hook.ts";

/// A provider's work-roots rule, as its startup configured it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ProviderWorkRoots {
    /// The configured roots, already expanded to absolute paths.
    pub(crate) roots: Vec<PathBuf>,
    /// The hook that makes Claude ask outside the roots.
    pub(crate) hook: WorkRootsHook,
}

/// Whether a provider's sessions in auto have their work-roots hook.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum WorkRootsHook {
    /// The provider needs none (Cursor: the Router's allowance decides).
    #[default]
    NotNeeded,
    /// Installed, with the Node that runs it.
    Ready(AskOutsideHook),
    /// Claude needs it and the Host couldn't set it up: an auto start is
    /// refused rather than run without it (D1 R18).
    Unavailable(String),
}

/// The installed hook and the Node that runs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AskOutsideHook {
    pub(crate) node: PathBuf,
    pub(crate) hook: PathBuf,
}

impl AskOutsideHook {
    /// The command Claude runs before a write or a shell call, naming this
    /// session's roots.
    pub(crate) fn command(&self, roots: &[PathBuf]) -> String {
        let roots = serde_json::to_string(
            &roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_owned());
        format!(
            "{} {} {}",
            shell_quoted(&self.node.to_string_lossy()),
            shell_quoted(&self.hook.to_string_lossy()),
            shell_quoted(&roots)
        )
    }
}

/// The Node a launch runs: the executable itself when it is `node`, or the
/// absolute Node a script's `#!` line names. `None` otherwise (a script run
/// through `env`, say), so Claude in auto is refused rather than unguarded.
pub(crate) fn launch_node(executable: &Path) -> Option<PathBuf> {
    let is_node = |path: &Path| path.file_name() == Some(std::ffi::OsStr::new("node"));
    if is_node(executable) {
        return Some(executable.to_path_buf());
    }
    let mut first_line = [0_u8; 256];
    let read = io::Read::read(&mut fs::File::open(executable).ok()?, &mut first_line).ok()?;
    let text = std::str::from_utf8(first_line.get(..read)?).ok()?;
    let interpreter = text
        .strip_prefix("#!")?
        .lines()
        .next()?
        .split_whitespace()
        .next()?;
    let interpreter = Path::new(interpreter);
    (interpreter.is_absolute() && is_node(interpreter)).then(|| interpreter.to_path_buf())
}

/// Writes the hook, owner-only, into `directory` and returns its path. The
/// file is replaced whole, so a session never reads a half-written hook.
pub(crate) fn install_work_roots_hook(directory: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let hook = directory.join(WORK_ROOTS_HOOK_FILE);
    let staged = directory.join(format!(
        ".{WORK_ROOTS_HOOK_FILE}.{}",
        durable_steps::Uuid::now_v7()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)?;
        io::Write::write_all(&mut file, WORK_ROOTS_HOOK_SOURCE.as_bytes())?;
    }
    fs::rename(&staged, &hook)?;
    Ok(hook)
}

/// A session's roots beyond its working and scratch folders: the configured
/// list, the project's tmp/, and the roots its creator named, once each.
pub(crate) fn session_work_roots(
    configured: &[PathBuf],
    working_directory: &Path,
    named: &[PathBuf],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for root in configured
        .iter()
        .cloned()
        .chain(std::iter::once(working_directory.join("tmp")))
        .chain(named.iter().cloned())
    {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

fn shell_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
#[path = "provider_work_roots_test.rs"]
mod tests;
