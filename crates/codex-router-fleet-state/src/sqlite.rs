//! Owns only fleet.sqlite and its one long-lived SQLx pool.
use crate::fleet_migrations::{self, FleetStateOpenError};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct FleetStateStore {
    pub(crate) pool: sqlx::SqlitePool,
    database_path: PathBuf,
}

fn private_root(root: &Path) -> Result<(), FleetStateOpenError> {
    match fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(root)
    {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(FleetStateOpenError::Unavailable),
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| FleetStateOpenError::Unavailable)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(FleetStateOpenError::Unavailable);
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<(), FleetStateOpenError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FleetStateOpenError::Unavailable)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(FleetStateOpenError::Unavailable);
    }
    Ok(())
}

impl FleetStateStore {
    /// Opens this Router's private fleet database and applies only this crate's
    /// migrations. The proxy's state.sqlite is never opened by this owner.
    pub async fn open_and_migrate(router_root: &Path) -> Result<Self, FleetStateOpenError> {
        private_root(router_root)?;
        let database_path = router_root.join(crate::FLEET_DATABASE_FILENAME);
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&database_path)
        {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(FleetStateOpenError::Unavailable),
        }
        private_file(&database_path)?;
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(false)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|_| FleetStateOpenError::Unavailable)?;
        if let Err(error) = fleet_migrations::migrate(&pool).await {
            pool.close().await;
            return Err(error);
        }
        Ok(Self {
            pool,
            database_path,
        })
    }

    /// Reads an existing fleet store without creating a file or migration history.
    pub async fn open_read_only(router_root: &Path) -> Result<Self, FleetStateOpenError> {
        let database_path = router_root.join(crate::FLEET_DATABASE_FILENAME);
        private_file(&database_path)?;
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .read_only(true)
            .create_if_missing(false)
            .pragma("query_only", "ON");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|_| FleetStateOpenError::Unavailable)?;
        let result = async {
            let mut connection = pool
                .acquire()
                .await
                .map_err(|_| FleetStateOpenError::Unavailable)?;
            fleet_migrations::validate_current(&mut connection).await
        }
        .await;
        if let Err(error) = result {
            pool.close().await;
            return Err(error);
        }
        Ok(Self {
            pool,
            database_path,
        })
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Waits for all checked-out connections before closing the owning pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;
