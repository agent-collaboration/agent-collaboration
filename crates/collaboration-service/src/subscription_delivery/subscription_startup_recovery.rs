//! Persisted reader discovery restores startup and starts missing writable-project owners.
use super::super::{
    ProjectSubscriptionRecord, SubscriptionWindowStore, subscription_facts::STORAGE_RETRY_INTERVAL,
    subscription_windows::StartupProjectDiscovery,
};
use super::{
    BoardError, HashMap, Identity, ProjectId, ReaderReconcilePurpose, ReaderScanScope,
    ServiceHandleLifetime, SubscriptionDeliveryService,
};
use std::collections::BTreeSet;

pub(super) enum StartupProjectOutcome {
    Retry,
    Restored,
    Unavailable,
}

impl SubscriptionDeliveryService {
    /// Start inherited readers at writable entry without refreshing live owners.
    pub async fn start_inherited_project_readers(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        self.require_started().await?;
        let probe_store = self
            .inner
            .board_availability
            .require_store()?
            .lock()
            .await
            .clone();
        let readers = match probe_store.discover_startup_project(project, &[]).await? {
            StartupProjectDiscovery::Readers(readers) => readers,
            StartupProjectDiscovery::Retry => {
                tracing::debug!(
                    project = project.as_str(),
                    "live subscription discovery awaits project storage"
                );
                return Err(BoardError::board_unavailable());
            }
            StartupProjectDiscovery::Unavailable => {
                tracing::warn!(
                    project = project.as_str(),
                    "live subscription project unavailable"
                );
                return Err(BoardError::board_unavailable());
            }
        };
        for reader in readers {
            self.ensure_owner_input(reader, ReaderScanScope::project(project.clone()), None)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn ensure_restored_owners(
        &self,
        records: Vec<ProjectSubscriptionRecord>,
        expiry_readers: Vec<(Identity, ProjectId)>,
    ) -> Result<(), BoardError> {
        let mut projects_by_reader: HashMap<Identity, BTreeSet<ProjectId>> = HashMap::new();
        for row in records {
            projects_by_reader
                .entry(row.record.reader().clone())
                .or_default()
                .insert(row.project);
        }
        for (reader, project) in expiry_readers {
            projects_by_reader
                .entry(reader)
                .or_default()
                .insert(project);
        }
        for (reader, projects) in projects_by_reader {
            if let Err(error) = self
                .reconcile_owned_scope(
                    reader,
                    ReaderScanScope::Projects(projects),
                    ReaderReconcilePurpose::Refresh,
                )
                .await
            {
                if self.inner.shutdown.is_cancelled() {
                    return Err(error);
                }
                // Restoration already succeeded. A replied failed pass still acknowledges
                // the existing owner's responsibility and its normal retry deadline.
                tracing::warn!(%error, "restored subscription reader acknowledged a failed pass; owner retries");
            }
        }
        Ok(())
    }

    pub(super) async fn restore_deferred_project(
        &self,
        probe_store: &SubscriptionWindowStore,
        project: &ProjectId,
    ) -> Result<StartupProjectOutcome, BoardError> {
        let known_readers: Vec<_> = self.inner.owners.lock().await.keys().cloned().collect();
        let readers = match probe_store
            .discover_startup_project(project, &known_readers)
            .await?
        {
            StartupProjectDiscovery::Retry => return Ok(StartupProjectOutcome::Retry),
            StartupProjectDiscovery::Unavailable => return Ok(StartupProjectOutcome::Unavailable),
            StartupProjectDiscovery::Readers(readers) => readers,
        };
        for reader in readers {
            let restored = self
                .reconcile_owned_scope(
                    reader,
                    ReaderScanScope::project(project.clone()),
                    ReaderReconcilePurpose::Restore,
                )
                .await?;
            if restored.unavailable.contains(project) {
                return Ok(StartupProjectOutcome::Unavailable);
            }
        }
        Ok(StartupProjectOutcome::Restored)
    }

    async fn restore_deferred_projects(
        &self,
        probe_store: &SubscriptionWindowStore,
        projects: &mut Vec<ProjectId>,
    ) -> Result<(), BoardError> {
        for project in projects.clone() {
            let outcome = self.restore_deferred_project(probe_store, &project).await?;
            match outcome {
                StartupProjectOutcome::Retry => continue,
                StartupProjectOutcome::Restored => {
                    tracing::info!(project=%project.as_str(),startup_recovery="resolved", "startup subscription project restored and acknowledged")
                }
                StartupProjectOutcome::Unavailable => {
                    tracing::warn!(project=%project.as_str(),startup_recovery="quarantined", "startup subscription project unavailable")
                }
            }
            // Commit each terminal disposition before a later project's shared failure.
            projects.retain(|pending| pending != &project);
        }
        Ok(())
    }

    pub(super) fn defer_startup_discovery(
        &self,
        probe_store: SubscriptionWindowStore,
        mut projects: Vec<ProjectId>,
    ) {
        if projects.is_empty() {
            return;
        }
        let service = Self {
            inner: self.inner.clone(),
            lifetime: ServiceHandleLifetime::Background,
        };
        self.inner.tasks.spawn(async move {
            tracing::debug!(startup_recovery="waiting", "startup discovery retry task retained");
            let mut shared_failure = false;
            while !projects.is_empty() {
                let deadline = service.inner.clock.monotonic_now() + STORAGE_RETRY_INTERVAL;
                let restored = tokio::select! {
                    biased;
                    () = service.inner.shutdown.cancelled() => break,
                    restored = async {
                        service.inner.clock.sleep_until(deadline).await;
                        service.restore_deferred_projects(&probe_store, &mut projects).await
                    } => restored,
                };
                match restored {
                    Ok(()) if shared_failure => {
                        tracing::info!(startup_recovery="resumed", "shared startup discovery storage recovered");
                        shared_failure=false;
                    }
                    Err(error) if !shared_failure => {
                        tracing::warn!(%error,startup_recovery="blocked", "shared startup discovery storage unavailable");
                        shared_failure=true;
                    }
                    _ => {}
                }
                #[cfg(test)]
                let _ = service.inner.startup_passes.send(projects.clone());
            }
            tracing::info!(startup_recovery="finished", "startup subscription discovery task drained");
        });
    }
}
