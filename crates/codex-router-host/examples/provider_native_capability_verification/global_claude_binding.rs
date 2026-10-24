//! Which Claude executable the Host binds for `claude-local`, recomputed independently.
//!
//! Mirrors `crates/codex-router-host/src/global_claude_executable.rs`
//! (`resolve_global_claude_executable`): the first absolute PATH entry, outside package
//! search directories before and after canonicalization, whose canonical `claude` is an
//! executable regular file that is neither a transient package nor a Claude SDK native
//! binary. Nothing is executed and PATH is never changed.
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

fn has_component(path: &Path, matches: impl Fn(&str) -> bool) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str().is_some_and(&matches))
}

fn is_transient_package_path(path: &Path) -> bool {
    has_component(path, |name| matches!(name, "_npx" | "dlx"))
}

fn is_package_search_path(path: &Path) -> bool {
    has_component(path, |name| name == "node_modules") || is_transient_package_path(path)
}

fn is_claude_sdk_path(path: &Path) -> bool {
    has_component(path, |name| {
        name.starts_with("claude-agent-sdk") || name.starts_with("@anthropic-ai+claude-agent-sdk")
    })
}

fn executable_by_effective_ids(path: &Path) -> bool {
    rustix::fs::accessat(
        rustix::fs::CWD,
        path,
        rustix::fs::Access::EXEC_OK,
        rustix::fs::AtFlags::EACCESS,
    )
    .is_ok()
}

/// The canonical global Claude executable the Host selects from `search_path`.
pub(crate) fn resolve_global_claude(search_path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(search_path).find_map(|directory| {
        if !directory.is_absolute() || is_package_search_path(&directory) {
            return None;
        }
        let canonical_directory = fs::canonicalize(directory).ok()?;
        if is_package_search_path(&canonical_directory) {
            return None;
        }
        let executable = fs::canonicalize(canonical_directory.join("claude")).ok()?;
        if is_transient_package_path(&executable) || is_claude_sdk_path(&executable) {
            return None;
        }
        let metadata = fs::metadata(&executable).ok()?;
        (metadata.is_file()
            && executable_by_effective_ids(&executable)
            && executable.to_str().is_some())
        .then_some(executable)
    })
}
