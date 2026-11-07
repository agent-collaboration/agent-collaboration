//! Files an older Router left in its collaboration directory that nothing reads any
//! more. Update step 0009 removes them, importing the approval routes first; the Host
//! refuses to start while one is still there.
//!
//! The enrollment reset journal and native schema export folders are not listed: the
//! running Host still writes both, so step 0009 removes only finished or leftover ones.
use std::{
    io,
    path::{Path, PathBuf},
};

/// Approval routes an older Router kept in JSON; step 0009 imports them into
/// `interaction.sqlite` before removing the file.
pub const LEGACY_APPROVAL_ROUTES_FILE: &str = "approval-routes.json";

const DEAD_FILE_NAMES: [&str; 6] = [
    "project-board.sqlite",
    "project-board.sqlite-wal",
    "project-board.sqlite-shm",
    "interaction-history.json",
    "approval-history.json",
    LEGACY_APPROVAL_ROUTES_FILE,
];
const CONTROL_SCHEMA_PREFIX: &str = "control-schema-";
const CONTROL_SCHEMA_SUFFIX: &str = ".json";
const SCHEMA_EXPORT_PREFIX: &str = "native-schema-export-";

/// Every dead file in the root's collaboration directory, in name order.
pub fn router_dead_files(router_root: &Path) -> io::Result<Vec<PathBuf>> {
    entries_named(router_root, |name, _| {
        DEAD_FILE_NAMES.contains(&name)
            || (name.starts_with(CONTROL_SCHEMA_PREFIX) && name.ends_with(CONTROL_SCHEMA_SUFFIX))
    })
}

/// Native schema export folders left in the collaboration directory, in name order.
pub fn router_schema_export_folders(router_root: &Path) -> io::Result<Vec<PathBuf>> {
    entries_named(router_root, |name, is_directory| {
        is_directory && name.starts_with(SCHEMA_EXPORT_PREFIX)
    })
}

fn entries_named(
    router_root: &Path,
    matches: impl Fn(&str, bool) -> bool,
) -> io::Result<Vec<PathBuf>> {
    let directory = crate::router_collaboration_directory(router_root);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if matches(name, entry.file_type()?.is_dir()) {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
#[path = "router_dead_files_test.rs"]
mod tests;
