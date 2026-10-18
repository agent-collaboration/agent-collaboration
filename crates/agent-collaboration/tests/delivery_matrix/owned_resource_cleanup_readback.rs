//! Cleanup assertions read M2 membership, watch and local-window owners together.
use super::super::proof_context::ProofResult;
use automation_storage::AutomationStore;
use collaboration_service::{ProjectStoreRegistry, SubscriptionWindowStore};
use message_board::{
    Identity, Record, SubscriptionScope, Thread, ThreadId, ThreadSubscriptionRecord,
};
use message_board_storage::ProjectStore;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

pub(crate) struct CleanupReadback {
    registry: Arc<ProjectStoreRegistry>,
    pub(crate) windows: SubscriptionWindowStore,
    automation: Arc<Mutex<AutomationStore>>,
}

impl CleanupReadback {
    pub(crate) async fn open(directory: &Path) -> ProofResult<Self> {
        let registry = Arc::new(ProjectStoreRegistry::new(directory));
        for project in registry.project_ids().await? {
            registry.existing_project(&project).await?;
        }
        let automation = Arc::new(Mutex::new(
            AutomationStore::open(&directory.join("automation.sqlite")).await?,
        ));
        let windows = SubscriptionWindowStore::new(registry.clone(), automation.clone());
        Ok(Self {
            registry,
            windows,
            automation,
        })
    }

    async fn containing_project(
        &self,
        thread: &ThreadId,
    ) -> ProofResult<Option<Arc<ProjectStore>>> {
        for project in self.registry.project_ids().await? {
            if let Some(store) = self.registry.existing_project(&project).await?
                && store.has_thread(thread).await?
            {
                return Ok(Some(store));
            }
        }
        Ok(None)
    }

    pub(crate) async fn subscription_state(
        &self,
        reader: &Identity,
        thread: &ThreadId,
    ) -> ProofResult<Option<ThreadSubscriptionRecord>> {
        if self.containing_project(thread).await?.is_none() {
            return Ok(None);
        }
        Ok(self
            .windows
            .get_thread_subscription_record(
                reader,
                &SubscriptionScope::thread(thread.clone()),
                chrono::Utc::now(),
            )
            .await?)
    }

    pub(crate) async fn thread_record(&self, thread: &ThreadId) -> ProofResult<Option<Thread>> {
        let Some(store) = self.containing_project(thread).await? else {
            return Ok(None);
        };
        let mut snapshot = store.snapshot().await?;
        let record = snapshot.record(&thread.clone().into()).await;
        snapshot.finish().await?;
        match record? {
            Some(Record::Thread(thread)) => Ok(Some(thread)),
            None => Ok(None),
            Some(_) => Err("thread readback returned another record kind".into()),
        }
    }

    pub(crate) async fn watch_active(
        &self,
        reader: &Identity,
        thread: &ThreadId,
    ) -> ProofResult<bool> {
        let store = self
            .containing_project(thread)
            .await?
            .ok_or("watch thread disappeared")?;
        let mut snapshot = store.snapshot().await?;
        let watch = snapshot.effective_thread_watch(thread, reader).await;
        snapshot.finish().await?;
        Ok(watch?.is_some())
    }

    pub(crate) async fn close(self) -> ProofResult<()> {
        drop(self.windows);
        let registry = Arc::try_unwrap(self.registry).map_err(|_| "readback registry retained")?;
        let registry_close = registry.close().await;
        let automation =
            Arc::try_unwrap(self.automation).map_err(|_| "readback automation retained")?;
        let automation_close = automation.into_inner().close().await;
        registry_close?;
        automation_close?;
        Ok(())
    }
}
