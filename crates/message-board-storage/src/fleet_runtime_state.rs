//! FleetCache owns local machine state as well as its central-derived projection.
use super::{FleetCache, RouterLocalStorageError};
use board_sync_protocol::FleetLinkRecord;
use chrono::Utc;
use fleet_sync_protocol::{Epoch, InstallAttemptId, NoticePosition};
use fleet_sync_session::ClientSessionState;
use message_board::ProjectId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[path = "fleet_runtime_import.rs"]
mod legacy;
#[cfg(test)]
#[path = "fleet_runtime_state_test.rs"]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotInstallAttempt {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub attempt: InstallAttemptId,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MachineNoticeProgress {
    handled_through: NoticePosition,
}

#[derive(Clone, Copy)]
enum RuntimeStateKind {
    Session,
    NoticeProgress,
    Link,
}
impl RuntimeStateKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Session => "machineSessionState",
            Self::NoticeProgress => "machineNoticeProgress",
            Self::Link => "centralLink",
        }
    }
}

impl FleetCache {
    pub async fn clear_machine_runtime(&self) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM fleet_runtime_state")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM fleet_snapshot_install_attempts")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn machine_session_state(
        &self,
    ) -> Result<Option<ClientSessionState>, RouterLocalStorageError> {
        read_runtime(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::Session,
        )
        .await
    }

    pub async fn save_machine_session_state(
        &self,
        state: &ClientSessionState,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        write_runtime(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::Session,
            state,
        )
        .await
    }

    pub async fn machine_notice_progress(&self) -> Result<NoticePosition, RouterLocalStorageError> {
        Ok(read_runtime::<MachineNoticeProgress>(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::NoticeProgress,
        )
        .await?
        .map_or(NoticePosition::new(0), |progress| progress.handled_through))
    }

    pub async fn save_machine_notice_progress(
        &self,
        handled_through: NoticePosition,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        write_runtime(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::NoticeProgress,
            &MachineNoticeProgress { handled_through },
        )
        .await
    }

    pub async fn snapshot_install_attempt(
        &self,
        project: &ProjectId,
        epoch: Epoch,
    ) -> Result<InstallAttemptId, RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let attempt = SnapshotInstallAttempt {
            project: project.clone(),
            epoch,
            attempt: InstallAttemptId::new(uuid::Uuid::now_v7())
                .map_err(|_| RouterLocalStorageError::InvalidRecord)?,
        };
        insert_attempt(&mut transaction, &attempt).await?;
        let project = project.as_str();
        let epoch = epoch.value().to_string();
        let id = sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM fleet_snapshot_install_attempts WHERE project = ? AND epoch = ?"#, project, epoch).fetch_one(&mut *transaction).await?;
        let attempt = uuid::Uuid::parse_str(&id)
            .ok()
            .and_then(|id| InstallAttemptId::new(id).ok())
            .filter(|attempt| attempt.to_string() == id)
            .ok_or(RouterLocalStorageError::InvalidRecordKey {
                field: "snapshotInstallAttempt",
            })?;
        transaction.commit().await?;
        Ok(attempt)
    }

    pub async fn fleet_link(&self) -> Result<Option<FleetLinkRecord>, RouterLocalStorageError> {
        let record: Option<FleetLinkRecord> = read_runtime(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::Link,
        )
        .await?;
        if record.as_ref().is_some_and(|record| !record.is_valid()) {
            return Err(RouterLocalStorageError::InvalidRecordKey {
                field: "centralLink",
            });
        }
        Ok(record)
    }

    pub async fn save_fleet_link(
        &self,
        record: &FleetLinkRecord,
    ) -> Result<(), RouterLocalStorageError> {
        if !record.is_valid() {
            return Err(RouterLocalStorageError::InvalidRecordKey {
                field: "centralLink",
            });
        }
        let _writer = self.database.writer.lock().await;
        write_runtime(
            &mut *self.database.pool.acquire().await?,
            RuntimeStateKind::Link,
            record,
        )
        .await
    }
}

async fn read_runtime<T: DeserializeOwned>(
    connection: &mut sqlx_turso::TursoConnection,
    kind: RuntimeStateKind,
) -> Result<Option<T>, RouterLocalStorageError> {
    let name = kind.as_str();
    let row = sqlx_turso::query!(r#"SELECT id AS "id!: String", record AS "record!: String" FROM fleet_runtime_state WHERE kind = ?"#, name).fetch_optional(connection).await?;
    row.map(|row| {
        message_board::RecordId::try_from(row.id)
            .map_err(|_| RouterLocalStorageError::InvalidRecordKey { field: name })?;
        serde_json::from_str(&row.record)
            .map_err(|_| RouterLocalStorageError::InvalidRecordKey { field: name })
    })
    .transpose()
}

async fn write_runtime<T: Serialize>(
    connection: &mut sqlx_turso::TursoConnection,
    kind: RuntimeStateKind,
    record: &T,
) -> Result<(), RouterLocalStorageError> {
    let id = uuid::Uuid::now_v7().to_string();
    let kind = kind.as_str();
    let record = serde_json::to_string(record)?;
    let now = Utc::now().to_rfc3339();
    sqlx_turso::query!("INSERT INTO fleet_runtime_state(id, kind, record, created_at, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(kind) DO UPDATE SET record = excluded.record, updated_at = excluded.updated_at", id, kind, record, now, now).execute(connection).await?;
    Ok(())
}

async fn insert_attempt(
    connection: &mut sqlx_turso::TursoConnection,
    record: &SnapshotInstallAttempt,
) -> Result<(), RouterLocalStorageError> {
    let id = record.attempt.to_string();
    let project = record.project.as_str();
    let epoch = record.epoch.value().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx_turso::query!("INSERT INTO fleet_snapshot_install_attempts(id, project, epoch, created_at, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(project, epoch) DO NOTHING", id, project, epoch, now, now).execute(connection).await?;
    Ok(())
}
