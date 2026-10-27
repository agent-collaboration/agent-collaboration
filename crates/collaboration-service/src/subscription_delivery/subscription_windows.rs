//! Join replicated membership to Router-local delivery progress, without project writes.
use crate::ProjectStoreRegistry;
use chrono::{DateTime, Utc};
use interaction_storage::{
    InteractionStore, SubscriptionDeliveryProgress, SubscriptionDeliveryProgressProps,
    SubscriptionWindowSelection,
};
use message_board::{
    BatchId, BoardError, EndReason, FeedContent, FeedCursor, FeedPosition, HandoffSeq, Identity,
    MAX_SUBSCRIPTION_NOTICE_ROOTS, PendingRootNotice, Placement, ProjectId, Record, RecordId,
    ResourceIdentity, SubscriptionBatch, SubscriptionBatchRootSettlement,
    SubscriptionBatchSettlement, SubscriptionDeliveryOutcome, SubscriptionGeneration,
    SubscriptionMembership, SubscriptionMode, SubscriptionRootRecord, SubscriptionRootRecordProps,
    SubscriptionScope, SubscriptionState, SubscriptionWindowId, ThreadId, ThreadRef,
    ThreadSubscriptionRecord, ThreadSubscriptionRecordProps, WhenIdle,
};
use std::{collections::HashSet, sync::Arc};
#[cfg(test)]
use tokio::sync::Mutex;
use tokio::sync::broadcast;

#[path = "follow_window_proposals.rs"]
mod follow_window_proposals;
#[path = "subscription_delivery_view.rs"]
mod subscription_delivery_view;
#[path = "subscription_window_lineage.rs"]
mod subscription_window_lineage;
#[path = "subscription_window_scan.rs"]
mod subscription_window_scan;
#[path = "subscription_window_selection.rs"]
mod subscription_window_selection;
pub use subscription_delivery_view::SubscriptionDeliveryView;
pub(super) use subscription_window_selection::WindowWrite;
#[path = "subscription_window_restore.rs"]
mod subscription_window_restore;
pub(in crate::subscription_delivery) use subscription_window_restore::StartupProjectDiscovery;
pub use subscription_window_restore::SubscriptionStartupRestoration;
#[path = "subscription_window_error.rs"]
mod subscription_window_error;
use subscription_window_error::{
    WindowStoreError, invalid_membership, invalid_window, project_registry_failure,
    project_storage_failure, shared_storage_failure, unavailable,
};

#[derive(Clone)]
pub struct SubscriptionWindowStore {
    registry: Arc<ProjectStoreRegistry>,
    interactions: Arc<InteractionStore>,
    #[cfg(test)]
    expiry_read_probe: Arc<Mutex<Option<Arc<follow_window_proposals::ExpiryReadProbe>>>>,
}
#[derive(Clone, Debug)]
pub struct ProjectSubscriptionRecord {
    pub project: ProjectId,
    pub record: ThreadSubscriptionRecord,
    pub delivery: SubscriptionDeliveryView,
    membership: SubscriptionMembership,
}

#[derive(Clone, Debug, Default)]
pub struct ReaderSubscriptionSnapshot {
    pub records: Vec<ProjectSubscriptionRecord>,
    pub unavailable: Vec<ProjectId>,
    pub(super) resolved_view_candidates: Vec<ProjectSubscriptionRecord>,
}
pub(super) struct MembershipProjectSelection {
    pub projects: Vec<ProjectId>,
    pub unavailable: Vec<ProjectId>,
}

struct MembershipProgress {
    membership: SubscriptionMembership,
    progress: SubscriptionDeliveryProgressProps,
    coverage: Vec<collaboration_mail_protocol::RootRange>,
}
impl SubscriptionWindowStore {
    #[must_use]
    pub fn new(registry: Arc<ProjectStoreRegistry>, interactions: Arc<InteractionStore>) -> Self {
        Self {
            registry,
            interactions,
            #[cfg(test)]
            expiry_read_probe: Arc::new(Mutex::new(None)),
        }
    }
    pub fn subscribe_activity(&self) -> broadcast::Receiver<ProjectId> {
        self.registry.subscribe_activity()
    }
    pub(super) async fn membership_projects(
        &self,
        reader: Option<&Identity>,
    ) -> Result<Vec<ProjectId>, BoardError> {
        Ok(self.select_membership_projects(reader).await?.projects)
    }
    pub(super) async fn select_membership_projects(
        &self,
        reader: Option<&Identity>,
    ) -> Result<MembershipProjectSelection, BoardError> {
        let mut selected = MembershipProjectSelection {
            projects: Vec::new(),
            unavailable: Vec::new(),
        };
        for project in self.registry.project_ids().await.map_err(unavailable)? {
            let probe = self.probe_membership_project(reader, &project).await;
            match self.contain_project_failure(probe).await? {
                Some(true) => selected.projects.push(project),
                Some(false) => {}
                None => selected.unavailable.push(project),
            }
        }
        Ok(selected)
    }
    async fn probe_membership_project(
        &self,
        reader: Option<&Identity>,
        project: &ProjectId,
    ) -> Result<bool, WindowStoreError> {
        let Some(store) = self
            .registry
            .existing_project(project)
            .await
            .map_err(|error| project_registry_failure(project, error))?
        else {
            return Ok(false);
        };
        store
            .has_subscription_memberships(reader)
            .await
            .map_err(|error| project_storage_failure(project, error))
    }
    pub(super) async fn memberships_in_projects(
        &self,
        reader: Option<&Identity>,
        projects: &[ProjectId],
    ) -> Result<Vec<SubscriptionMembership>, WindowStoreError> {
        let mut memberships = Vec::new();
        for project in projects {
            let result = self.project_memberships(reader, project).await;
            let rows = self.contain_project_failure(result).await?;
            if let Some(rows) = rows {
                memberships.extend(rows);
            }
        }
        Ok(memberships)
    }
    async fn project_memberships(
        &self,
        reader: Option<&Identity>,
        project: &ProjectId,
    ) -> Result<Vec<SubscriptionMembership>, WindowStoreError> {
        let Some(store) = self
            .registry
            .existing_project(project)
            .await
            .map_err(|error| project_registry_failure(project, error))?
        else {
            return Ok(Vec::new());
        };
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        let rows = read
            .subscription_memberships(reader)
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        read.finish()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        let mut memberships = Vec::new();
        for membership in rows {
            match membership {
                Ok(membership) => memberships.push(membership),
                Err(error) if reader.is_none() => {
                    tracing::warn!(%error, "invalid subscription membership skipped during reader discovery");
                }
                Err(error) => return Err(project_storage_failure(project, error)),
            }
        }
        Ok(memberships)
    }
    async fn load(
        &self,
        membership: SubscriptionMembership,
    ) -> Result<MembershipProgress, WindowStoreError> {
        self.load_with_preventions(membership, &[]).await
    }
    async fn load_with_preventions(
        &self,
        membership: SubscriptionMembership,
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<MembershipProgress, WindowStoreError> {
        let project = membership
            .envelope
            .scope
            .project
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        let progress = self
            .interactions
            .subscription_progress(project, &membership.envelope.id, membership.activated_at())
            .await
            .map_err(|error| match error {
                interaction_storage::StorageError::InvalidRecord
                | interaction_storage::StorageError::InvalidStoredRecord => {
                    WindowStoreError::from(invalid_window())
                }
                other => shared_storage_failure(other),
            })?;
        let mut progress = match progress {
            Some(progress) => {
                let mut props: SubscriptionDeliveryProgressProps = progress.into();
                if props.generation != membership.generation() {
                    // Policy changed within this activation: keep ranges and delivery positions,
                    // release old selections, and retain generation fencing for their receipts.
                    props.generation = membership.generation();
                    for root in &mut props.roots {
                        if let Some(window) = &mut root.window {
                            window.selected = None;
                        }
                    }
                }
                props
            }
            _ => {
                // A successor derives debt from the activation floor and frozen handoffs.
                let scan_through = membership.activated_at();
                SubscriptionDeliveryProgressProps {
                    project: project.clone(),
                    membership: membership.envelope.id.clone(),
                    generation: membership.generation(),
                    activated_at: membership.activated_at(),
                    scan_through,
                    last_outcome: None,
                    roots: Vec::new(),
                }
            }
        };
        self.reconcile_progress_lineage(&mut progress).await?;
        let coverage = self.follow_coverage(&membership, preventions).await?;
        for root in &mut progress.roots {
            if root.window.as_ref().is_some_and(|window| {
                coverage.iter().any(|range| {
                    range.root() == &root.thread_id
                        && range.from().position <= window.through.position
                        && range.through().position >= window.from.position
                })
            }) {
                root.window = None;
                root.delivered_through = membership.activated_at();
                progress.scan_through = membership.activated_at();
            }
        }
        if membership
            .ended()
            .is_some_and(|ended| ended.reason != EndReason::Resolved)
        {
            // Cancellation revokes pending notices; an already selected effect still settles.
            for root in &mut progress.roots {
                if root
                    .window
                    .as_ref()
                    .is_some_and(|window| window.selected.is_none())
                {
                    root.window = None;
                }
            }
        }
        Ok(MembershipProgress {
            membership,
            progress,
            coverage,
        })
    }
    async fn contain_project_failure<TValue>(
        &self,
        result: Result<TValue, WindowStoreError>,
    ) -> Result<Option<TValue>, WindowStoreError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(
                error @ (WindowStoreError::Shared { .. } | WindowStoreError::RetryProject { .. }),
            ) => Err(error),
            Err(WindowStoreError::Project { project, error }) => {
                tracing::warn!(?project, %error, "subscription recovery unavailable for this project");
                self.registry.mark_unavailable(&project).await;
                Ok(None)
            }
        }
    }
    async fn save(
        &self,
        progress: &SubscriptionDeliveryProgressProps,
    ) -> Result<(), WindowStoreError> {
        let validated =
            SubscriptionDeliveryProgress::new(progress.clone()).map_err(shared_storage_failure)?;
        self.interactions
            .put_subscription_progress(&validated)
            .await
            .map_err(shared_storage_failure)
    }
    fn view(
        &self,
        loaded: MembershipProgress,
        now: DateTime<Utc>,
    ) -> Result<ProjectSubscriptionRecord, BoardError> {
        let delivery = SubscriptionDeliveryView::default();
        let notices_owed = loaded
            .membership
            .ended()
            .is_none_or(|ended| ended.reason == EndReason::Resolved)
            && (loaded
                .progress
                .roots
                .iter()
                .any(|root| root.window.is_some()));
        let state = loaded.membership.state_at(now, None, notices_owed);
        let roots = loaded
            .progress
            .roots
            .iter()
            .filter_map(|root| {
                state
                    .is_delivery_eligible()
                    .then_some(root)
                    .and_then(|root| root.window.as_ref().map(|window| (root, window)))
            })
            .map(|(root, window)| {
                SubscriptionRootRecord::new(SubscriptionRootRecordProps {
                    thread_id: root.thread_id.clone(),
                    opened_at: window.opened_at,
                    last_arrival_at: window.last_arrival_at,
                    pending_count: window.post_count,
                    held_since: None,
                    next_retry_at: None,
                    retry_attempts: 0,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(unavailable)?;
        let project = loaded.progress.project.clone();
        let record = ThreadSubscriptionRecord::new(ThreadSubscriptionRecordProps {
            reader: loaded.membership.reader().clone(),
            scope: loaded.membership.scope().clone(),
            policy: loaded.membership.policy().clone(),
            state,
            renewed_at: loaded.membership.renewed_at(),
            expires_at: loaded.membership.expires_at(),
            ended_at: state.end_reason().map(|_| {
                loaded
                    .membership
                    .ended()
                    .map_or(loaded.membership.expires_at(), |ended| ended.at)
            }),
            generation: loaded.membership.generation(),
            last_outcome: delivery
                .last_outcome
                .clone()
                .or(loaded.progress.last_outcome),
            roots,
        })
        .map_err(unavailable)?;
        Ok(ProjectSubscriptionRecord {
            project,
            record,
            delivery,
            membership: loaded.membership,
        })
    }
    pub async fn list_reader_subscriptions(
        &self,
        reader: &Identity,
        now: DateTime<Utc>,
    ) -> Result<ReaderSubscriptionSnapshot, BoardError> {
        let selected = self.select_membership_projects(Some(reader)).await?;
        let mut snapshot = self
            .project_subscription_records(reader, now, &selected.projects, &[])
            .await?;
        snapshot.unavailable.extend(selected.unavailable);
        snapshot.unavailable.sort();
        snapshot.unavailable.dedup();
        self.observe_subscription_deliveries(&mut snapshot).await?;
        Ok(snapshot)
    }
    pub(super) async fn project_subscription_records(
        &self,
        reader: &Identity,
        now: DateTime<Utc>,
        projects: &[ProjectId],
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<ReaderSubscriptionSnapshot, WindowStoreError> {
        let mut snapshot = ReaderSubscriptionSnapshot::default();
        for project in projects {
            let result = async {
                let mut records = Vec::new();
                let mut candidates = Vec::new();
                for membership in self.project_memberships(Some(reader), project).await? {
                    let record = self.view(
                        self.load_with_preventions(membership, preventions).await?,
                        now,
                    )?;
                    if record.record.state().is_delivery_eligible() {
                        records.push(record);
                    } else if record.record.state()
                        == (SubscriptionState::Ended {
                            reason: EndReason::Resolved,
                        })
                        && record.record.roots().is_empty()
                    {
                        candidates.push(record);
                    }
                }
                Ok((records, candidates))
            }
            .await;
            match self.contain_project_failure(result).await? {
                Some((records, candidates)) => {
                    snapshot.records.extend(records);
                    snapshot.resolved_view_candidates.extend(candidates);
                }
                None => snapshot.unavailable.push(project.clone()),
            }
        }
        Ok(snapshot)
    }
    pub async fn get_thread_subscription_record(
        &self,
        reader: &Identity,
        scope: &SubscriptionScope,
        now: DateTime<Utc>,
    ) -> Result<Option<ThreadSubscriptionRecord>, BoardError> {
        let project = match scope {
            SubscriptionScope::Project { project_id } => project_id.clone(),
            SubscriptionScope::Thread { thread_id } => {
                self.thread_project(&thread_id.clone().into()).await?
            }
        };
        self.subscription_record_in_project(reader, scope, &project, now)
            .await
    }
    pub(super) async fn subscription_record_in_project(
        &self,
        reader: &Identity,
        scope: &SubscriptionScope,
        project: &ProjectId,
        now: DateTime<Utc>,
    ) -> Result<Option<ThreadSubscriptionRecord>, BoardError> {
        let Some(store) = self
            .registry
            .existing_project(project)
            .await
            .map_err(unavailable)?
        else {
            return Ok(None);
        };
        let mut read = store.snapshot().await.map_err(unavailable)?;
        let member = read
            .membership(reader, scope)
            .await
            .map_err(invalid_membership)?;
        read.finish().await.map_err(unavailable)?;
        match member {
            Some(member) => self
                .view(self.load(member).await?, now)
                .map(|row| Some(row.record)),
            None => Ok(None),
        }
    }
    pub async fn due_subscription_roots(
        &self,
        reader: &Identity,
        now: DateTime<Utc>,
    ) -> Result<Vec<ThreadId>, BoardError> {
        let projects = self.membership_projects(Some(reader)).await?;
        self.due_project_subscription_roots(reader, now, &projects, &[])
            .await
    }
    pub(super) async fn due_project_subscription_roots(
        &self,
        reader: &Identity,
        now: DateTime<Utc>,
        projects: &[ProjectId],
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<Vec<ThreadId>, BoardError> {
        let mut roots = Vec::new();
        for membership in self.memberships_in_projects(Some(reader), projects).await? {
            let loaded = self.load_with_preventions(membership, preventions).await?;
            let owed = loaded
                .progress
                .roots
                .iter()
                .any(|root| root.window.is_some());
            if !loaded
                .membership
                .state_at(now, None, owed)
                .is_delivery_eligible()
                || loaded.membership.policy().mode() == SubscriptionMode::Off
            {
                continue;
            }
            for root in &loaded.progress.roots {
                let Some(window) = &root.window else { continue };
                if window.selected.is_some() {
                    continue;
                }
                let timing = loaded.membership.policy().timing();
                let quiet = window.last_arrival_at
                    + chrono::Duration::seconds(
                        i64::try_from(timing.quiet_seconds()).map_err(unavailable)?,
                    );
                let cap = window.opened_at
                    + chrono::Duration::seconds(
                        i64::try_from(timing.cap_seconds()).map_err(unavailable)?,
                    );
                if quiet.min(cap) <= now {
                    roots.push(root.thread_id.clone());
                }
            }
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }
}
fn membership_mode_off_or_expired(membership: &SubscriptionMembership, now: DateTime<Utc>) -> bool {
    membership.policy().mode() == SubscriptionMode::Off
        || membership.ended().is_none() && membership.expires_at() <= now
}
