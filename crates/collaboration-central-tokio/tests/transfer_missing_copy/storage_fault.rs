//! Public stopped Registry reads and exact owned-file removal/restoration.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_lease::EpochGrant;
use sqlx_core::connection::{ConnectOptions, Connection};
use std::path::Path;

struct HistoryRead<'connection>(&'connection mut sqlx_turso::TursoConnection);
impl TransactionScope for HistoryRead<'_> {
    type Database = sqlx_turso::Turso;
    type Executor<'query>
        = &'query mut sqlx_turso::TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}
#[derive(Debug)]
pub(super) struct RegistryRead {
    pub(super) lease: Option<LeaseStatus>,
    pub(super) grants: Vec<Option<EpochGrant>>,
    pub(super) outcome: Option<HolderChangeAnswer>,
}
pub(super) async fn read_registry(
    root: &Path,
    request: &HolderChangeRequest,
) -> ConformanceResult<RegistryRead> {
    let database = match request {
        HolderChangeRequest::Takeover { database, .. } => *database,
        _ => return Err("file-loss proof requires its exact Takeover".into()),
    };
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let proof = async {
        let mut transaction = connection.begin().await?;
        let mut scope = HistoryRead(&mut transaction);
        let at: chrono::DateTime<chrono::Utc> = "2030-03-17T17:46:40Z".parse()?;
        let lease = fleet_sync_lease::lease_status(
            &mut scope,
            database,
            SyncTime::from_unix_millis(at.timestamp_millis()),
        )
        .await?;
        let mut grants = Vec::new();
        for epoch in 1..=6 {
            grants.push(
                fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(epoch)?).await?,
            );
        }
        let outcome = fleet_sync_lease::holder_change_answer(&mut scope, request).await?;
        transaction.rollback().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(RegistryRead {
            lease,
            grants,
            outcome,
        })
    }
    .await;
    let closed = connection.close().await;
    let result = proof?;
    closed?;
    Ok(result)
}

pub(super) async fn read_absent_copy(
    root: &Path,
    database: DatabaseId,
) -> ConformanceResult<CopyStatusAnswer> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{database}.db")))
        .read_only(true)
        .connect()
        .await?;
    let proof = async {
        let mut transaction = connection.begin().await?;
        let status = fleet_sync_receiver::copy_status(&mut HistoryRead(&mut transaction)).await?;
        transaction.rollback().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(status)
    }
    .await;
    let closed = connection.close().await;
    let status = proof?;
    closed?;
    Ok(status)
}

pub(super) struct CopyBackup {
    directory: tempfile::TempDir,
    files: Vec<(String, Vec<u8>)>,
}
impl CopyBackup {
    pub(super) fn remove_from(root: &Path, database: DatabaseId) -> ConformanceResult<Self> {
        let directory = tempfile::tempdir()?;
        let mut files = Vec::new();
        for suffix in [".db", ".db-wal", ".db-shm"] {
            let name = format!("copy-{database}{suffix}");
            let path = root.join(&name);
            if suffix == ".db" || path.exists() {
                let contents = std::fs::read(&path)?;
                std::fs::rename(&path, directory.path().join(&name))?;
                files.push((name, contents));
            }
        }
        Ok(Self { directory, files })
    }
    pub(super) fn restore_to(self, root: &Path) -> ConformanceResult<()> {
        // All owning runtimes are stopped. Remove only the newly created empty
        // files for this one test-owned Copy, then restore its retained bytes.
        let (main, _) = self
            .files
            .first()
            .ok_or("Copy backup omitted its main database")?;
        for suffix in ["", "-wal", "-shm"] {
            let path = root.join(format!("{main}{suffix}"));
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        for (name, contents) in &self.files {
            let saved = self.directory.path().join(name);
            if std::fs::read(&saved)? != *contents {
                return Err("retained original Copy bytes changed while absent".into());
            }
            std::fs::rename(saved, root.join(name))?;
        }
        Ok(())
    }
}
