//! A real database rejection replaces the old read-only JSON-directory fixture.
use super::*;
use sqlx::Connection;

pub(super) enum CacheWriteTarget {
    InstallAttempt,
    NoticeProgress,
}
pub(super) struct CacheWriteFailure {
    connection: sqlx_turso::TursoConnection,
}

impl CacheWriteFailure {
    pub(super) async fn hold(
        root: &Path,
        target: CacheWriteTarget,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut connection = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(root.join("fleet-cache.sqlite")),
        )
        .await?;
        let trigger = match target {
            CacheWriteTarget::InstallAttempt => {
                "CREATE TRIGGER fixture_refuse_cache_write BEFORE INSERT ON fleet_snapshot_install_attempts BEGIN SELECT RAISE(ABORT,'fixture install attempt write failure'); END"
            }
            CacheWriteTarget::NoticeProgress => {
                "CREATE TRIGGER fixture_refuse_cache_write BEFORE INSERT ON fleet_runtime_state WHEN NEW.kind = 'machineNoticeProgress' BEGIN SELECT RAISE(ABORT,'fixture notice progress write failure'); END"
            }
        };
        sqlx::raw_sql(trigger).execute(&mut connection).await?;
        Ok(Self { connection })
    }
    pub(super) async fn restore(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query("DROP TRIGGER fixture_refuse_cache_write")
            .execute(&mut self.connection)
            .await?;
        self.connection.close().await?;
        Ok(())
    }
}
