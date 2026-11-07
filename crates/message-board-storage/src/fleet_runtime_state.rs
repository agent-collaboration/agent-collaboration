//! Router-owned session, link observation and unfinished installs in FleetCache.
use super::{FleetCache, RouterLocalStorageError};
use chrono::{DateTime, Utc};
use fleet_protocol::{Epoch, InstallAttemptId, NoticePosition};
use fleet_session::ClientSessionState;
use message_board::ProjectId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[path = "fleet_runtime_import.rs"]
mod legacy;
#[path = "fleet_link_rows.rs"]
mod links;
#[cfg(feature = "test-support")]
#[path = "fleet_notice_progress_observer.rs"]
mod observation;
#[path = "fleet_session_rows.rs"]
mod sessions;
#[cfg(feature = "test-support")]
pub use observation::{MachineNoticeProgressObservation, observe_machine_notice_progress};
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

impl FleetCache {
    pub async fn clear_machine_runtime(&self) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM fleet_session")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM fleet_link_state")
            .execute(&mut *transaction)
            .await?;
        // The attempt belongs to an unfinished project install, not enrollment.
        transaction.commit().await?;
        Ok(())
    }

    pub async fn snapshot_install_attempt(
        &self,
        project: &ProjectId,
        epoch: Epoch,
    ) -> Result<InstallAttemptId, RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = read_attempt(&mut transaction, project, epoch).await?;
        let attempt = match existing {
            Some(attempt) => attempt,
            None => {
                let record = SnapshotInstallAttempt {
                    project: project.clone(),
                    epoch,
                    attempt: InstallAttemptId::new(uuid::Uuid::now_v7())
                        .map_err(|_| invalid("installAttempt.id"))?,
                };
                insert_attempt(&mut transaction, &record).await?;
                record.attempt
            }
        };
        transaction.commit().await?;
        Ok(attempt)
    }

    pub async fn complete_snapshot_install(
        &self,
        project: &ProjectId,
        epoch: Epoch,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        // Validate the row even on deletion; corrupted identities are not silently retired.
        let _current = read_attempt(&mut transaction, project, epoch).await?;
        let project = project.as_str();
        let epoch = epoch.value().to_string();
        sqlx_turso::query!(
            "DELETE FROM fleet_install_attempts WHERE project_id = ? AND epoch = ?",
            project,
            epoch
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn read_attempt(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    epoch: Epoch,
) -> Result<Option<InstallAttemptId>, RouterLocalStorageError> {
    let project = project.as_str();
    let rows = sqlx_turso::query!(r#"SELECT id AS "id!: String", project_id AS "project_id!: String", epoch AS "epoch!: String", created_at AS "created_at!: String", updated_at AS "updated_at!: String" FROM fleet_install_attempts WHERE project_id = ?"#, project)
        .fetch_all(connection).await?;
    let mut selected = None;
    for row in rows {
        validate_id(&row.id, "installAttempt.id")?;
        ProjectId::try_from(row.project_id).map_err(|_| invalid("installAttempt.projectId"))?;
        let stored_epoch = Epoch::new(parse_u64(&row.epoch, "installAttempt.epoch")?)
            .map_err(|_| invalid("installAttempt.epoch"))?;
        validate_times(&row.created_at, &row.updated_at)?;
        let attempt = InstallAttemptId::new(
            uuid::Uuid::parse_str(&row.id).map_err(|_| invalid("installAttempt.id"))?,
        )
        .map_err(|_| invalid("installAttempt.id"))?;
        if stored_epoch == epoch {
            selected = Some(attempt);
        }
    }
    Ok(selected)
}

async fn insert_attempt(
    connection: &mut sqlx_turso::TursoConnection,
    record: &SnapshotInstallAttempt,
) -> Result<(), RouterLocalStorageError> {
    let id = record.attempt.to_string();
    let project = record.project.as_str();
    let epoch = record.epoch.value().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx_turso::query!("INSERT INTO fleet_install_attempts(id, project_id, epoch, created_at, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(project_id, epoch) DO NOTHING", id, project, epoch, now, now).execute(connection).await?;
    Ok(())
}

fn invalid(field: &'static str) -> RouterLocalStorageError {
    RouterLocalStorageError::InvalidRecordKey { field }
}
fn parse_u64(value: &str, field: &'static str) -> Result<u64, RouterLocalStorageError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| number.to_string() == value)
        .ok_or_else(|| invalid(field))
}
fn validate_id(value: &str, field: &'static str) -> Result<(), RouterLocalStorageError> {
    message_board::RecordId::try_from(value.to_owned())
        .map(|_| ())
        .map_err(|_| invalid(field))
}
fn parse_time(value: &str, field: &'static str) -> Result<DateTime<Utc>, RouterLocalStorageError> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| invalid(field))?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(invalid(field));
    }
    Ok(parsed.with_timezone(&Utc))
}
fn validate_times(created: &str, updated: &str) -> Result<(), RouterLocalStorageError> {
    parse_time(created, "createdAt")?;
    parse_time(updated, "updatedAt")?;
    Ok(())
}
