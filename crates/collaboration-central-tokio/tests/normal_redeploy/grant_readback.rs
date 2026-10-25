//! Read historical grants through the public library API after server shutdown.
use super::*;
use durable_steps::storage::TransactionScope;
use sqlx_core::connection::{ConnectOptions, Connection};

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

pub(super) async fn assert_no_granted_epoch_three(
    root: &std::path::Path,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let proof = async {
        let mut transaction = connection.begin().await?;
        let mut scope = HistoryRead(&mut transaction);
        let skipped = fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(3)?).await?;
        transaction.rollback().await?;
        if skipped.is_some() {
            return Err(format!("stale Preview nevertheless granted epoch3: {skipped:?}").into());
        }
        println!("NORMAL_REDEPLOY_STAGE stopped_public_epoch_holder_three_never_granted passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = connection.close().await;
    proof?;
    closed?;
    Ok(())
}
