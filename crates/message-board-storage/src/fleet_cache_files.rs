//! File privacy belongs to FleetCache, around its existing native pool opening.
use crate::RouterLocalStorageError;
use std::{
    fs::{File, OpenOptions, Permissions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) struct PrivateFleetCacheFiles {
    database: PathBuf,
    wal: PathBuf,
    coordination: PathBuf,
}

fn refused() -> RouterLocalStorageError {
    RouterLocalStorageError::PrivateFleetCache
}

fn owned_file(file: &File, owner: u32) -> Result<(), RouterLocalStorageError> {
    let metadata = file.metadata().map_err(|_| refused())?;
    if !metadata.is_file() || metadata.uid() != owner {
        return Err(refused());
    }
    Ok(())
}

fn tighten(path: &Path, create: bool) -> Result<(), RouterLocalStorageError> {
    let no_follow = i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits()).map_err(|_| refused())?;
    let created = if create {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(no_follow)
            .open(path)
        {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
            Err(_) => return Err(refused()),
        }
    } else {
        None
    };
    let file = match created {
        Some(file) => file,
        None => OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(no_follow)
            .open(path)
            .map_err(|_| refused())?,
    };
    owned_file(&file, rustix::process::geteuid().as_raw())?;
    file.set_permissions(Permissions::from_mode(0o600))
        .map_err(|_| refused())?;
    let metadata = file.metadata().map_err(|_| refused())?;
    let current = std::fs::symlink_metadata(path).map_err(|_| refused())?;
    if metadata.permissions().mode() & 0o777 != 0o600
        || !current.is_file()
        || current.ino() != metadata.ino()
        || current.dev() != metadata.dev()
        || current.uid() != metadata.uid()
        || current.permissions().mode() & 0o777 != 0o600
    {
        return Err(refused());
    }
    Ok(())
}

impl PrivateFleetCacheFiles {
    pub(super) fn prepare(path: &Path) -> Result<Self, RouterLocalStorageError> {
        let mut wal = path.as_os_str().to_os_string();
        wal.push("-wal");
        let mut coordination = path.as_os_str().to_os_string();
        coordination.push("-tshm");
        let files = Self {
            database: path.to_owned(),
            wal: wal.into(),
            coordination: coordination.into(),
        };
        tighten(&files.database, true)?;
        tighten(&files.wal, true)?;
        match std::fs::symlink_metadata(&files.coordination) {
            Ok(_) => tighten(&files.coordination, false)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(refused()),
        }
        Ok(files)
    }

    pub(super) fn verify_after_native_open(&self) -> Result<(), RouterLocalStorageError> {
        tighten(&self.database, false)?;
        tighten(&self.wal, false)?;
        // Turso cannot open an empty pre-created -tshm. On first creation this
        // coordination-only file briefly has its native default mode. Tighten
        // it before exposing FleetCache; no bearer row is served until then.
        match std::fs::symlink_metadata(&self.coordination) {
            Ok(_) => tighten(&self.coordination, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(refused()),
        }
    }
}

#[cfg(test)]
#[path = "fleet_cache_files_unit_test.rs"]
mod tests;
