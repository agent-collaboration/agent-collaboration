//! Select only the private peer registry established by the actual normal-HOME Host.
use super::ProofResult;
use serde_json::Value;
use std::{
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
};

pub(super) fn selected_directory(root: &Path) -> ProofResult<PathBuf> {
    let context: Value =
        serde_json::from_slice(&std::fs::read(root.join("debug-host-context.json"))?)?;
    if context.get("kind").and_then(Value::as_str) != Some("debugHostPrepared") {
        return Err("peer fixture requires the normal-HOME acceptance Host context".into());
    }
    let selected = context
        .get("peerRegistryDirectory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or("Host did not select a task-private peer registry")?;
    let expected = root.join("peer-registry");
    let metadata = std::fs::symlink_metadata(&selected)?;
    if !selected.is_absolute()
        || !metadata.file_type().is_dir()
        || metadata.uid() != std::fs::metadata(root)?.uid()
        || metadata.permissions().mode() & 0o077 != 0
        || selected.canonicalize()? != expected.canonicalize()?
        || selected.canonicalize()?.parent() != Some(root.canonicalize()?.as_path())
    {
        return Err("Host peer registry must be its owner-private peer-registry child".into());
    }
    Ok(selected)
}

#[cfg(test)]
#[path = "peer_registry_test.rs"]
pub(crate) mod tests;
