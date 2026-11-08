//! Read native membership, watches and local windows with the persisted installation peer.
use super::super::proof_context::ProofResult;
use collaboration_runtime::NativeProjectStore;
use collaboration_runtime::ProjectStoreRegistry;
use collaboration_runtime::load_installation_peer;
use collaboration_service::SubscriptionWindowStore;
use interaction_storage::InteractionStore;
use message_board::{
    Identity, Record, SubscriptionScope, Thread, ThreadId, ThreadSubscriptionRecord,
};
use std::{path::Path, sync::Arc};

pub(crate) struct CleanupReadback {
    registry: Arc<ProjectStoreRegistry>,
    pub(crate) windows: SubscriptionWindowStore,
    interactions: Arc<InteractionStore>,
}

impl CleanupReadback {
    pub(crate) async fn open(directory: &Path) -> ProofResult<Self> {
        let registry = Arc::new(ProjectStoreRegistry::new(
            directory,
            Some(load_installation_peer(directory)?),
            Arc::new(runtime_primitives::SystemClock),
        ));
        for project in registry.project_ids().await? {
            registry.existing_project(&project).await?;
        }
        let interactions =
            Arc::new(InteractionStore::open(directory.join("interaction.sqlite")).await?);
        let windows = SubscriptionWindowStore::new(registry.clone(), interactions.clone());
        Ok(Self {
            registry,
            windows,
            interactions,
        })
    }

    async fn containing_project(
        &self,
        thread: &ThreadId,
    ) -> ProofResult<Option<Arc<NativeProjectStore>>> {
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
        let interactions =
            Arc::try_unwrap(self.interactions).map_err(|_| "readback interactions retained")?;
        let interactions_close = interactions.close().await;
        registry_close?;
        interactions_close?;
        Ok(())
    }
}
