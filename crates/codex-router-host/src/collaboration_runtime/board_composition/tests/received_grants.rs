//! Two independent Router roots share the actual central Registry and Copy.
use super::*;

pub(super) async fn wait_published_project(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    epoch: u64,
) -> Result<Arc<collaboration_runtime::NativeProjectStore>, Box<dyn std::error::Error + Send + Sync>>
{
    timeout(BOUND, async {
        loop {
            if let Ok(Some(store)) = registry.existing_project(project).await
                && matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual.value() == epoch)
            { return Ok(store); }
            tokio::task::yield_now().await;
        }
    }).await?
}
