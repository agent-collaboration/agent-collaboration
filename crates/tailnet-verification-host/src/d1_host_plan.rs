//! D1-only allocation inputs; never relax the existing Tailnet plan's guards.
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::{
    driver_error::{DriverError, DriverResult},
    host_plan::require_deadline,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct D1NativeArtifact {
    pub path: PathBuf,
    pub sha256: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum D1AuthPolicy {
    AwaitingOwner {},
    SharedDebugProxy { origin: String },
    DedicatedDebugAccounts {},
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct D1NativeReadinessPlan {
    pub repository: PathBuf,
    pub run_root: PathBuf,
    pub native: D1NativeArtifact,
    pub auth_policy: D1AuthPolicy,
    pub deadline: DateTime<FixedOffset>,
}

fn absolute_without_traversal(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}

pub fn require_unaliased_ancestors(path: &Path) -> DriverResult<()> {
    let mut walked = PathBuf::new();
    for component in path.components() {
        walked.push(component);
        match std::fs::symlink_metadata(&walked) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DriverError::Invalid("D1 allocation contains a symlink"));
            }
            Ok(metadata) if metadata.uid() != rustix::process::getuid().as_raw() => {
                // Root/system ancestors belong to the OS; owned state begins at tmp/.
                if path.starts_with(&walked) && walked == path {
                    return Err(DriverError::Invalid("D1 allocation has another owner"));
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

impl D1NativeReadinessPlan {
    pub fn validate(&self) -> DriverResult<()> {
        require_deadline(&self.deadline)?;
        if !absolute_without_traversal(&self.repository)
            || self.repository.canonicalize()? != self.repository
            || self.repository
                != Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()?
        {
            return Err(DriverError::Invalid(
                "D1 repository is not the driver's worktree",
            ));
        }
        let allocation = self.repository.join("tmp/d1-verify/allocation");
        if let Ok(metadata) = allocation.symlink_metadata()
            && (!metadata.is_dir()
                || metadata.uid() != rustix::process::getuid().as_raw()
                || metadata.mode() & 0o077 != 0)
        {
            return Err(DriverError::Invalid(
                "D1 allocation parent must be owner-private",
            ));
        }
        if !absolute_without_traversal(&self.run_root)
            || self.run_root.parent() != Some(allocation.as_path())
            || self.run_root.file_name().is_none()
            || self.run_root.symlink_metadata().is_ok()
        {
            return Err(DriverError::Invalid(
                "D1 run root must be a fresh allocation child",
            ));
        }
        require_unaliased_ancestors(&self.run_root)?;
        if !absolute_without_traversal(&self.native.path)
            || self.native.sha256.len() != 64
            || !self
                .native
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.native.version.is_empty()
        {
            return Err(DriverError::Invalid(
                "declared D1 native artifact is invalid",
            ));
        }
        if let D1AuthPolicy::SharedDebugProxy { origin } = &self.auth_policy {
            let parsed = reqwest::Url::parse(origin)
                .map_err(|_| DriverError::Invalid("D1 proxy origin must be loopback HTTP"))?;
            if parsed.scheme() != "http"
                || parsed.host_str() != Some("127.0.0.1")
                || parsed.port().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
                || parsed.path() != "/"
            {
                return Err(DriverError::Invalid(
                    "D1 proxy origin must be loopback HTTP",
                ));
            }
        }
        Ok(())
    }
}

pub fn read_d1_plan(path: &Path) -> DriverResult<D1NativeReadinessPlan> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.len() > 1024 * 1024
    {
        return Err(DriverError::Invalid(
            "D1 plan must be bounded and owner-private",
        ));
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    let plan: D1NativeReadinessPlan = serde_json::from_slice(&bytes)?;
    plan.validate()?;
    Ok(plan)
}

#[cfg(test)]
#[path = "d1_host_plan_test.rs"]
mod tests;
