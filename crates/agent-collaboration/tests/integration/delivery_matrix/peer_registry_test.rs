use super::*;
use serde_json::json;
use std::os::unix::fs::{DirBuilderExt as _, symlink};

fn context(root: &Path, selected: Option<&Path>) -> ProofResult<()> {
    std::fs::write(
        root.join("debug-host-context.json"),
        serde_json::to_vec(&json!({
            "kind":"debugHostPrepared", "peerRegistryDirectory":selected,
        }))?,
    )?;
    Ok(())
}

pub(crate) fn actual_private_host_selection_is_consumed_without_a_fake_home() -> ProofResult<()> {
    let root = tempfile::tempdir()?;
    let registry = root.path().join("peer-registry");
    std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
    context(root.path(), Some(&registry))?;
    if selected_directory(root.path())? != registry {
        return Err("selected registry differs from the actual private Host selection".into());
    }
    if root.path().join("home").exists() {
        return Err("registry selection created an unexpected fake home".into());
    }
    Ok(())
}

pub(crate) fn absent_foreign_public_and_symlinked_registry_are_refused() -> ProofResult<()> {
    let root = tempfile::tempdir()?;
    let foreign = tempfile::tempdir()?;
    context(root.path(), None)?;
    if selected_directory(root.path()).is_ok() {
        return Err("absent Host registry selection was accepted".into());
    }
    context(root.path(), Some(foreign.path()))?;
    if selected_directory(root.path()).is_ok() {
        return Err("foreign Host registry selection was accepted".into());
    }
    let registry = root.path().join("peer-registry");
    symlink(foreign.path(), &registry)?;
    context(root.path(), Some(&registry))?;
    if selected_directory(root.path()).is_ok() {
        return Err("symlinked Host registry selection was accepted".into());
    }
    std::fs::remove_file(&registry)?;
    std::fs::DirBuilder::new().mode(0o755).create(&registry)?;
    std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o755))?;
    if selected_directory(root.path()).is_ok() {
        return Err("public Host registry selection was accepted".into());
    }
    Ok(())
}
