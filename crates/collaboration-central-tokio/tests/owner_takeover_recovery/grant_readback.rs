//! Independent historical assertions while the owning server is stopped.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_lease::EpochGrant;
use sqlx_core::connection::{ConnectOptions, Connection};

// The public lease read needs only an executor, not a Host or an object runtime.
struct HistoryReadScope<'connection>(&'connection mut sqlx_turso::TursoConnection);
impl TransactionScope for HistoryReadScope<'_> {
    type Database = sqlx_turso::Turso;
    type Executor<'query>
        = &'query mut sqlx_turso::TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

pub(super) async fn assert_history(
    root: &Path,
    database: DatabaseId,
    recovered: bool,
) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let proof = async {
        let mut transaction = connection.begin().await?;
        let mut scope = HistoryReadScope(&mut transaction);
        let first = fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(1)?).await?;
        let skipped = fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(2)?).await?;
        let final_grant = fleet_sync_lease::epoch_holder(&mut scope, database, Epoch::new(3)?).await?;
        transaction.rollback().await?;
        let start: chrono::DateTime<chrono::Utc> = START.parse()?;
        let expected_first = EpochGrant {
            epoch: Epoch::new(1)?, holder: PeerId::try_from(OLD_PEER.to_owned())?,
            granted_at: SyncTime::from_unix_millis(start.timestamp_millis()),
            ended_at: if recovered {Some(position(1,2)?)} else {None},
        };
        let expected_final = if recovered { Some(EpochGrant {
            epoch: Epoch::new(3)?, holder: PeerId::try_from(TARGET_PEER.to_owned())?,
            granted_at: SyncTime::from_unix_millis(start.timestamp_millis()+1000), ended_at: None,
        }) } else {None};
        if first != Some(expected_first) || skipped.is_some() || final_grant != expected_final {
            return Err(format!("public epoch history lost exact grants1/3 or ever granted skipped epoch2: first={first:?}, skipped={skipped:?}, final={final_grant:?}").into());
        }
        println!("TAKEOVER_RECOVERY_STAGE stopped_public_history_exact_grants_and_no_epoch2 recovered={recovered} passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = connection.close().await;
    proof?;
    closed?;
    Ok(())
}
