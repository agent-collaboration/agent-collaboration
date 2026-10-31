//! The apparatus writes only within its declared private checkout directory.
use serde::Deserialize;
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub type RigResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigInputs {
    pub repository: PathBuf,
    pub directory: PathBuf,
    pub claude_acp: Option<PathBuf>,
}

impl RigInputs {
    pub fn read(path: &Path) -> RigResult<Self> {
        let inputs: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        inputs.validate()?;
        Ok(inputs)
    }

    fn validate(&self) -> RigResult {
        let compiled_repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        if self.repository != compiled_repository
            || self.repository.canonicalize()? != self.repository
        {
            return Err("plan must name the checkout that built this apparatus".into());
        }
        require_private_directory(&self.directory)?;
        let temporary = self.repository.join("tmp");
        if self.directory == temporary || !self.directory.starts_with(&temporary) {
            return Err("apparatus directory must be inside this checkout's tmp".into());
        }
        // macOS Unix socket addresses are bounded; leave room for Host endpoint names.
        if self.directory.as_os_str().len() > 78 {
            return Err("apparatus directory too long for independent Host sockets".into());
        }
        if let Some(adapter) = &self.claude_acp
            && (!adapter.is_absolute() || !adapter.is_file())
        {
            return Err("Claude ACP adapter must be a qualified absolute executable".into());
        }
        Ok(())
    }
}

pub fn require_private_directory(directory: &Path) -> RigResult {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || directory.canonicalize()? != directory
    {
        return Err("canonical owner-private apparatus directory required".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn private_directory_refuses_world_access_and_symlink_aliases() -> RigResult {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&scratch)?;
        let root = tempfile::tempdir_in(scratch)?;
        let canonical = root.path().canonicalize()?;
        std::fs::set_permissions(&canonical, std::fs::Permissions::from_mode(0o700))?;
        require_private_directory(&canonical)?;
        std::fs::set_permissions(&canonical, std::fs::Permissions::from_mode(0o755))?;
        assert!(require_private_directory(&canonical).is_err());
        std::fs::set_permissions(&canonical, std::fs::Permissions::from_mode(0o700))?;
        let alias = canonical.join("alias");
        symlink(&canonical, &alias)?;
        assert!(require_private_directory(&alias).is_err());
        Ok(())
    }

    #[test]
    fn plan_refuses_unknown_fields_before_opening_any_runtime() {
        let decoded = serde_json::from_str::<RigInputs>(
            r#"{"repository":"/owned","directory":"/owned/tmp/r","claude_acp":null,"production":true}"#,
        );
        assert!(decoded.is_err());
    }
}
