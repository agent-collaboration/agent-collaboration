//! Cross-process lock for pooled credential creation, migration, and writes.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use fs2::FileExt;

use crate::model::SecretStoreError;

const STORE_LOCK_FILE: &str = ".store.lock";

/// Lock mode for the pooled credential store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreLockMode {
    /// Allows concurrent login and refresh writes.
    Shared,
    /// Excludes all pooled writers while creating key state or migrating files.
    Exclusive,
}

/// Holds the process lock until dropped.
#[derive(Debug)]
pub struct CredentialStoreLock {
    file: File,
}

impl CredentialStoreLock {
    /// Acquires the store lock file under the router-owned secrets directory.
    pub fn acquire(
        secret_root: &Path,
        mode: CredentialStoreLockMode,
    ) -> Result<Self, SecretStoreError> {
        let path = secret_root.join(STORE_LOCK_FILE);
        reject_symlink(&path)?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|source| SecretStoreError::CredentialStoreLock {
                path: path.clone(),
                source,
            })?;
        let attempted = match mode {
            CredentialStoreLockMode::Shared => FileExt::try_lock_shared(&file),
            CredentialStoreLockMode::Exclusive => FileExt::try_lock_exclusive(&file),
        };
        match attempted {
            Ok(()) => Ok(()),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                #[cfg(test)]
                crate::lock_contention_observation::report_contention();
                match mode {
                    CredentialStoreLockMode::Shared => FileExt::lock_shared(&file),
                    CredentialStoreLockMode::Exclusive => FileExt::lock_exclusive(&file),
                }
            }
            Err(error) => Err(error),
        }
        .map_err(|source| SecretStoreError::CredentialStoreLock { path, source })?;
        Ok(Self { file })
    }
}

impl Drop for CredentialStoreLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn reject_symlink(path: &Path) -> Result<(), SecretStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SecretStoreError::SymlinkPath {
            path: path.to_path_buf(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SecretStoreError::Filesystem {
            path: path.to_path_buf(),
            source,
        }),
    }
}
