//! Scoped startup discovery retains transient project identities until an owner can be created.
use super::{
    BoardError, DateTime, HashSet, Identity, ProjectId, ProjectSubscriptionRecord,
    SubscriptionWindowStore, Utc, WindowStoreError, unavailable,
};

#[derive(Default)]
pub struct SubscriptionStartupRestoration {
    pub records: Vec<ProjectSubscriptionRecord>,
    pub retry_projects: Vec<ProjectId>,
    pub(in crate::subscription_delivery) expiry_readers: Vec<(Identity, ProjectId)>,
    unavailable: HashSet<ProjectId>,
}
impl SubscriptionStartupRestoration {
    fn failed(&self, project: &ProjectId) -> bool {
        self.retry_projects.contains(project) || self.unavailable.contains(project)
    }
}

pub(in crate::subscription_delivery) enum StartupProjectDiscovery {
    Readers(Vec<Identity>),
    Retry,
    Unavailable,
}

impl SubscriptionWindowStore {
    pub(in crate::subscription_delivery) async fn discover_startup_project(
        &self,
        project: &ProjectId,
        known_readers: &[Identity],
    ) -> Result<StartupProjectDiscovery, BoardError> {
        let result = async {
            if !self.probe_membership_project(None, project).await? {
                return Ok(Vec::new());
            }
            let mut readers: HashSet<_> = self
                .project_memberships(None, project)
                .await?
                .into_iter()
                .map(|membership| membership.reader().clone())
                .collect();
            for reader in known_readers {
                if !readers.contains(reader)
                    && self.probe_membership_project(Some(reader), project).await?
                {
                    readers.insert(reader.clone());
                }
            }
            Ok(readers.into_iter().collect())
        }
        .await;
        match result {
            Ok(readers) => Ok(StartupProjectDiscovery::Readers(readers)),
            Err(WindowStoreError::RetryProject { .. }) => Ok(StartupProjectDiscovery::Retry),
            Err(WindowStoreError::Project { project, .. }) => {
                self.registry.mark_unavailable(&project).await;
                Ok(StartupProjectDiscovery::Unavailable)
            }
            Err(error) => Err(error.into()),
        }
    }

    // This is the sole reset/rescan rule. Startup has no live effects yet; deferred
    // recovery invokes it from the reader's serialized pass after any effect settles.
    async fn restore_project_reader(
        &mut self,
        reader: &Identity,
        project: &ProjectId,
        now: DateTime<Utc>,
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<(), WindowStoreError> {
        let memberships = self.project_memberships(Some(reader), project).await?;
        for membership in &memberships {
            let mut loaded = self
                .load_with_preventions(membership.clone(), preventions)
                .await?;
            for root in &mut loaded.progress.roots {
                if let Some(window) = &mut root.window {
                    window.selected = None;
                }
            }
            self.save(&loaded.progress).await?;
        }
        self.rescan_memberships(reader, now, memberships, preventions)
            .await?;
        Ok(())
    }
    pub(in crate::subscription_delivery) async fn restore_project_subscription_windows(
        &mut self,
        reader: &Identity,
        now: DateTime<Utc>,
        projects: &[ProjectId],
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<(), WindowStoreError> {
        for project in projects {
            let result = self
                .restore_project_reader(reader, project, now, preventions)
                .await;
            self.contain_project_failure(result).await?;
        }
        Ok(())
    }
    pub async fn restore_active_and_draining(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<SubscriptionStartupRestoration, BoardError> {
        let projects = self.registry.project_ids().await.map_err(unavailable)?;
        self.restore_startup_projects(now, &projects).await
    }

    pub(in crate::subscription_delivery) async fn restore_startup_projects(
        &mut self,
        now: DateTime<Utc>,
        projects: &[ProjectId],
    ) -> Result<SubscriptionStartupRestoration, BoardError> {
        let mut restored = SubscriptionStartupRestoration::default();
        let mut available = Vec::new();
        let mut readers = HashSet::new();
        for project in projects {
            let discovery = async {
                if !self.probe_membership_project(None, project).await? {
                    return Ok(None);
                }
                let memberships = self.project_memberships(None, project).await?;
                let expiry_readers: HashSet<_> = memberships
                    .iter()
                    .filter(|member| member.ended().is_none() && now >= member.expires_at())
                    .map(|member| member.reader().clone())
                    .collect();
                let project_readers = memberships
                    .into_iter()
                    .map(|membership| membership.reader().clone())
                    .collect::<HashSet<_>>();
                Ok(Some((project_readers, expiry_readers)))
            }
            .await;
            if let Some(Some((discovered, expiry_readers))) = self
                .startup_project_result(discovery, &mut restored)
                .await?
            {
                available.push(project.clone());
                readers.extend(discovered);
                restored.expiry_readers.extend(
                    expiry_readers
                        .into_iter()
                        .map(|reader| (reader, project.clone())),
                );
            }
        }
        // Validate only this pass's discovered project set. A deferred project's probe
        // is never repeated for every healthy reader, and retries never rediscover the fleet.
        for reader in readers {
            for project in &available {
                if restored.failed(project) {
                    continue;
                }
                let load = async {
                    self.restore_project_reader(&reader, project, now, &[])
                        .await?;
                    self.project_subscription_records(
                        &reader,
                        now,
                        std::slice::from_ref(project),
                        &[],
                    )
                    .await
                }
                .await;
                if let Some(snapshot) = self.startup_project_result(load, &mut restored).await? {
                    for unavailable in snapshot.unavailable {
                        restored.unavailable.insert(unavailable.clone());
                        tracing::warn!(project = %unavailable.as_str(), startup_recovery = "quarantined", "startup subscription project unavailable");
                    }
                    restored.records.extend(snapshot.records);
                }
            }
        }
        restored.records.retain(|row| {
            !restored.unavailable.contains(&row.project)
                && !restored.retry_projects.contains(&row.project)
        });
        restored.expiry_readers.retain(|(_, project)| {
            !restored.unavailable.contains(project) && !restored.retry_projects.contains(project)
        });
        Ok(restored)
    }

    async fn startup_project_result<TValue>(
        &self,
        result: Result<TValue, WindowStoreError>,
        restored: &mut SubscriptionStartupRestoration,
    ) -> Result<Option<TValue>, BoardError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(WindowStoreError::Shared { error, diagnostic }) => {
                if let Some(diagnostic) = diagnostic {
                    tracing::warn!(error = %diagnostic, "startup shared subscription storage unavailable");
                }
                Err(error)
            }
            Err(WindowStoreError::RetryProject {
                project,
                diagnostic,
                ..
            }) => {
                tracing::warn!(project = %project.as_str(), error = %diagnostic, startup_recovery = "deferred", "startup subscription discovery deferred");
                if !restored.retry_projects.contains(&project) {
                    restored.retry_projects.push(project);
                }
                Ok(None)
            }
            Err(WindowStoreError::Project { project, error }) => {
                self.registry.mark_unavailable(&project).await;
                tracing::warn!(project = %project.as_str(), %error, startup_recovery = "quarantined", "startup subscription project unavailable");
                restored.unavailable.insert(project);
                Ok(None)
            }
        }
    }
}
