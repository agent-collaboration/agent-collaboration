//! Move preparation closes admission, drains readers and checkpoints the existing native writer.
use super::{ProjectCommitGuard, ProjectStore};
use crate::{ProjectLedgerReader, ProjectStorageError};
use sqlx::Pool;
use sqlx::{Column, Row};
use sqlx_turso::{Turso, TursoConnection};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::{OwnedRwLockReadGuard, RwLock, watch};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectMoveStatus {
    Open,
    Moving,
    MoveAsidePending,
    Moved,
}
#[derive(Clone)]
pub(crate) struct ProjectReadAccess {
    status: watch::Sender<ProjectMoveStatus>,
    active_reads: Arc<RwLock<()>>,
}
impl ProjectReadAccess {
    pub(super) fn new() -> Self {
        Self {
            status: watch::channel(ProjectMoveStatus::Open).0,
            active_reads: Arc::new(RwLock::new(())),
        }
    }
    pub(super) fn require_open(&self) -> Result<(), ProjectStorageError> {
        if *self.status.borrow() == ProjectMoveStatus::Open {
            Ok(())
        } else {
            Err(ProjectStorageError::MoveAsidePending {
                reason: "project file is moving".to_owned(),
            })
        }
    }
    pub(super) async fn enter(&self) -> Result<OwnedRwLockReadGuard<()>, ProjectStorageError> {
        self.require_open()?;
        let guard = self.active_reads.clone().read_owned().await;
        self.require_open()?;
        Ok(guard)
    }
}
struct PendingMoveGuard {
    status: watch::Sender<ProjectMoveStatus>,
}
impl Drop for PendingMoveGuard {
    fn drop(&mut self) {
        let moving = *self.status.borrow() == ProjectMoveStatus::Moving;
        if moving {
            self.status
                .send_replace(ProjectMoveStatus::MoveAsidePending);
        }
    }
}
impl ProjectStore {
    /// The caller keeps the already committed project's writer mutex through publication.
    pub async fn move_aside(
        &self,
        commit: &ProjectCommitGuard,
        target: &Path,
    ) -> Result<(), ProjectStorageError> {
        if commit.project_id() != self.project_id() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if *self.read_access.status.borrow() == ProjectMoveStatus::Moved {
            return Err(ProjectStorageError::MoveAsidePending {
                reason: "project file already moved".to_owned(),
            });
        }
        self.read_access
            .status
            .send_replace(ProjectMoveStatus::Moving);
        let _pending = PendingMoveGuard {
            status: self.read_access.status.clone(),
        };
        self.complete_move_aside(target).await.map_err(|error| {
            ProjectStorageError::MoveAsidePending {
                reason: error.to_string(),
            }
        })
    }
    async fn complete_move_aside(&self, target: &Path) -> Result<(), ProjectStorageError> {
        let _reads = tokio::time::timeout(
            Duration::from_secs(5),
            self.read_access.active_reads.clone().write_owned(),
        )
        .await
        .map_err(|_| ProjectStorageError::MoveAsidePending {
            reason: "in-flight reads did not drain".to_owned(),
        })?;
        let mut connection = self.authoritative.acquire().await?;
        let mut checkpoint_complete = false;
        for attempt in 0..3_u32 {
            let row = read_wal_checkpoint(&mut connection).await?;
            if row.busy == 0 && row.checkpointed == row.log {
                checkpoint_complete = true;
                break;
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(20_u64 << attempt)).await;
            }
        }
        if !checkpoint_complete {
            return Err(ProjectStorageError::MoveAsidePending {
                reason: "native checkpoint remained busy".to_owned(),
            });
        }
        drop(connection);
        self.close_connections().await;
        let wal = std::path::PathBuf::from(format!("{}-wal", self.path.display()));
        match tokio::fs::metadata(&wal).await {
            Ok(metadata) if metadata.len() != 0 => {
                return Err(ProjectStorageError::MoveAsidePending {
                    reason: "committed WAL data remains after checkpoint".to_owned(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // Verify the closed source before moving as well as the read-only target afterward.
        let reader = ProjectLedgerReader::open(&self.path, self.project_id.clone()).await?;
        reader.close().await;
        tokio::fs::rename(&self.path, target).await?;
        let published = async {
            sync_parent(target).await?;
            let reader = ProjectLedgerReader::open(target, self.project_id.clone()).await?;
            reader.close().await;
            Ok::<(), ProjectStorageError>(())
        }
        .await;
        if let Err(error) = published {
            tokio::fs::rename(target, &self.path).await?;
            sync_parent(&self.path).await?;
            return Err(error);
        }
        self.read_access
            .status
            .send_replace(ProjectMoveStatus::Moved);
        Ok(())
    }
}
async fn sync_parent(path: &Path) -> Result<(), ProjectStorageError> {
    let parent = path.parent().ok_or(ProjectStorageError::InvalidSchema)?;
    tokio::fs::File::open(parent).await?.sync_all().await?;
    Ok(())
}

struct StoredWalCheckpoint {
    busy: i64,
    log: i64,
    checkpointed: i64,
}
async fn read_wal_checkpoint(
    connection: &mut TursoConnection,
) -> Result<StoredWalCheckpoint, ProjectStorageError> {
    let rows = sqlx::query::<Turso>("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_all(connection)
        .await?;
    if rows.len() != 1 {
        return Err(ProjectStorageError::InvalidSchema);
    }
    let row = rows.first().ok_or(ProjectStorageError::InvalidSchema)?;
    let mut columns: Vec<_> = row.columns().iter().map(Column::name).collect();
    columns.sort_unstable();
    if columns != ["busy", "checkpointed", "log"] {
        return Err(ProjectStorageError::InvalidSchema);
    }
    let result = StoredWalCheckpoint {
        busy: row.try_get("busy")?,
        log: row.try_get("log")?,
        checkpointed: row.try_get("checkpointed")?,
    };
    if result.busy < 0 || result.log < 0 || result.checkpointed < 0 {
        return Err(ProjectStorageError::InvalidSchema);
    }
    Ok(result)
}
pub(crate) async fn verify_move_integrity(pool: &Pool<Turso>) -> Result<(), ProjectStorageError> {
    let rows = sqlx::query::<Turso>("PRAGMA integrity_check")
        .fetch_all(pool)
        .await?;
    if rows.len() != 1 {
        return Err(ProjectStorageError::InvalidSchema);
    }
    let row = rows.first().ok_or(ProjectStorageError::InvalidSchema)?;
    let columns: Vec<_> = row.columns().iter().map(Column::name).collect();
    if columns != ["integrity_check"] {
        return Err(ProjectStorageError::InvalidSchema);
    }
    let result = StoredIntegrityCheck {
        status: row.try_get("integrity_check")?,
    };
    if result.status != "ok" {
        return Err(ProjectStorageError::InvalidSchema);
    }
    Ok(())
}
struct StoredIntegrityCheck {
    status: String,
}
