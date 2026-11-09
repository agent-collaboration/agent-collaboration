//! Read-only fixture observations share the canonical machine-session decoder.
use super::{NoticePosition, RouterLocalStorageError, sessions};
use sqlx::{ConnectOptions, Connection};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineNoticeProgressObservation {
    NotYet,
    Present { handled_through: NoticePosition },
}

/// Observe handled progress without creating or migrating its canonical store.
pub async fn observe_machine_notice_progress(
    path: &Path,
) -> Result<MachineNoticeProgressObservation, RouterLocalStorageError> {
    if !tokio::fs::try_exists(path)
        .await
        .map_err(|error| RouterLocalStorageError::Database(sqlx::Error::Io(error)))?
    {
        return Ok(MachineNoticeProgressObservation::NotYet);
    }
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?;
    let observed = sessions::read_session(&mut connection).await;
    let closed = connection.close().await;
    let observed = observed?;
    closed?;
    Ok(
        observed.map_or(MachineNoticeProgressObservation::NotYet, |row| {
            MachineNoticeProgressObservation::Present {
                handled_through: row.handled_through,
            }
        }),
    )
}

#[cfg(test)]
#[path = "fleet_notice_progress_observer_test.rs"]
mod tests;
