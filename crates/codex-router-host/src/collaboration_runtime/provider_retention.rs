//! Host-owned provider operation retention and its shutdown-bound maintenance task.
use super::{CollaborationRuntime, io};
use collaboration_runtime::provider_operation_retention_cutoff_ms;

impl CollaborationRuntime {
    pub async fn configure_provider_operation_retention(
        &mut self,
        retention_days: Option<std::num::NonZeroU32>,
        clock: std::sync::Arc<dyn runtime_primitives::Clock>,
    ) -> io::Result<u64> {
        let now_ms = clock.now().unix_millis();
        let cutoff_ms = provider_operation_retention_cutoff_ms(now_ms, retention_days)
            .map_err(io::Error::other)?;
        let protected = self
            .external_provider_supervisor
            .as_ref()
            .map(|supervisor| supervisor.live_operation_ids())
            .unwrap_or_default();
        let Some(store) = &self.provider_store else {
            return Ok(0);
        };
        let pruned = {
            let mut store = store.lock().await;
            let pruned = store
                .prune_terminal_before(cutoff_ms, &protected)
                .await
                .map_err(io::Error::other)?;
            store
                .incremental_vacuum_step()
                .await
                .map_err(io::Error::other)?;
            pruned
        };
        let store = std::sync::Arc::clone(store);
        let supervisor = self.external_provider_supervisor.clone();
        let shutdown = self.shutdown.clone();
        self.provider_retention = Some(tokio::spawn(async move {
            let period = std::time::Duration::from_secs(24 * 60 * 60);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ms = clock.now().unix_millis();
                        let Ok(cutoff_ms) = provider_operation_retention_cutoff_ms(now_ms, retention_days) else {
                            tracing::warn!("provider-operation retention clock is outside the supported range");
                            continue;
                        };
                        let protected = supervisor.as_ref().map(|value| value.live_operation_ids()).unwrap_or_default();
                        let mut store = store.lock().await;
                        if store.prune_terminal_before(cutoff_ms, &protected).await.is_ok() {
                            let _ = store.incremental_vacuum_step().await;
                        }
                    }
                }
            }
        }));
        Ok(pruned)
    }
}
