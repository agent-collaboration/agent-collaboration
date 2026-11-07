//! Independent readback of the committed attempt identities, without selecting a new one.
use super::*;
use fleet_sync_protocol::Epoch;
use message_board_storage::SnapshotInstallAttempt;
use sqlx::Connection;
type AttemptKey = (std::path::PathBuf, ProjectId, Epoch);
type ObservationResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(in crate::collaboration_runtime::board_composition) fn attempt_key(
    root: &Path,
    project: &ProjectId,
    epoch: Epoch,
) -> AttemptKey {
    (root.to_owned(), project.clone(), epoch)
}

pub(in crate::collaboration_runtime::board_composition) async fn attempt_record(
    key: &AttemptKey,
) -> ObservationResult<Option<SnapshotInstallAttempt>> {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(key.0.join("fleet-cache.sqlite"))
            .read_only(true),
    )
    .await?;
    let row = sqlx::query_as::<_, StoredAttempt>("SELECT id, project, epoch FROM fleet_snapshot_install_attempts WHERE project=? AND epoch=?")
        .bind(key.1.as_str()).bind(key.2.value().to_string()).fetch_optional(&mut connection).await?;
    connection.close().await?;
    row.map(|row| -> ObservationResult<_> {
        Ok(SnapshotInstallAttempt {
            project: ProjectId::try_from(row.project)?,
            epoch: Epoch::new(row.epoch.parse()?)?,
            attempt: fleet_sync_protocol::InstallAttemptId::new(uuid::Uuid::parse_str(&row.id)?)?,
        })
    })
    .transpose()
}

#[derive(sqlx::FromRow)]
struct StoredAttempt {
    id: String,
    project: String,
    epoch: String,
}

pub(in crate::collaboration_runtime::board_composition) async fn attempt_bytes(
    key: &AttemptKey,
) -> ObservationResult<Vec<u8>> {
    Ok(serde_json::to_vec(
        &attempt_record(key)
            .await?
            .ok_or("missing committed attempt")?,
    )?)
}

pub(in crate::collaboration_runtime::board_composition) async fn attempt_count(
    root: &Path,
) -> ObservationResult<usize> {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(root.join("fleet-cache.sqlite"))
            .read_only(true),
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_snapshot_install_attempts")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    Ok(usize::try_from(count)?)
}
