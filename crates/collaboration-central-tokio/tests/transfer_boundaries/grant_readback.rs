//! Public historical-grant reads on the stopped owning Registry, with no SQL knowledge.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_lease::EpochGrant;
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
pub(super) async fn original_only(
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
        let at: chrono::DateTime<chrono::Utc> = "2030-03-17T17:46:40Z".parse()?;
        let expected = EpochGrant {
            epoch: Epoch::new(1)?,
            holder: PeerId::try_from(fixture::OLD_PEER.to_owned())?,
            granted_at: SyncTime::from_unix_millis(at.timestamp_millis()),
            ended_at: None,
        };
        if fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(1)?).await?
            != Some(expected)
        {
            return Err("transfer refusal changed its actual original grant".into());
        }
        for epoch in 2..=6 {
            if fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(epoch)?)
                .await?
                .is_some()
            {
                return Err("transfer refusal nevertheless granted another epoch".into());
            }
        }
        transaction.rollback().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = connection.close().await;
    proof?;
    closed?;
    Ok(())
}
