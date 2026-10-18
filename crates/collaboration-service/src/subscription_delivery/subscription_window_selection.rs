//! Select and settle Router-local windows without writing replicated membership.
use super::*;

impl SubscriptionWindowStore {
    pub async fn thread_project(&self, thread: &ThreadRef) -> Result<ProjectId, BoardError> {
        self.registry
            .thread_project(thread)
            .await
            .map_err(unavailable)?
            .ok_or_else(|| {
                let mut error = BoardError::resource_not_found(ResourceIdentity::Thread {
                    thread_id: thread.thread.clone(),
                });
                error.message = "Thread not found in open local projects; a thread held elsewhere needs its project.".to_owned();
                error
            })
    }
    pub async fn select_subscription_notice(
        &mut self,
        reader: &Identity,
        threads: &[ThreadId],
        now: DateTime<Utc>,
        maximum_root_notice_bytes: usize,
    ) -> Result<(SubscriptionBatch, SubscriptionBatchSettlement), BoardError> {
        let projects = self.membership_projects(Some(reader)).await?;
        self.select_project_subscription_notice(
            reader,
            threads,
            now,
            maximum_root_notice_bytes,
            &projects,
        )
        .await
    }
    pub(in crate::service_identity::subscription_delivery) async fn select_project_subscription_notice(
        &mut self,
        reader: &Identity,
        threads: &[ThreadId],
        now: DateTime<Utc>,
        maximum_root_notice_bytes: usize,
        projects: &[ProjectId],
    ) -> Result<(SubscriptionBatch, SubscriptionBatchSettlement), BoardError> {
        self.select_window_notice(
            reader,
            threads,
            now,
            maximum_root_notice_bytes,
            WindowSelectionPurpose::Notice,
            projects,
        )
        .await
    }
    async fn select_window_notice(
        &mut self,
        reader: &Identity,
        threads: &[ThreadId],
        now: DateTime<Utc>,
        maximum_root_notice_bytes: usize,
        purpose: WindowSelectionPurpose,
        projects: &[ProjectId],
    ) -> Result<(SubscriptionBatch, SubscriptionBatchSettlement), BoardError> {
        let batch_id = BatchId::generate();
        let mut batch = SubscriptionBatch {
            batch_id: batch_id.clone(),
            held: false,
            held_since: None,
            draining: false,
            roots: Vec::new(),
        };
        let mut settlement = SubscriptionBatchSettlement { roots: Vec::new() };
        let mut progress_changes = Vec::new();
        for membership in self.memberships_in_projects(Some(reader), projects).await? {
            let mut loaded = self.load(membership).await?;
            let owed = loaded
                .progress
                .roots
                .iter()
                .any(|root| root.window.is_some());
            let state = loaded.membership.state_at(now, None, owed);
            if !state.is_delivery_eligible()
                || loaded.membership.policy().mode() == SubscriptionMode::Off
                || matches!(purpose, WindowSelectionPurpose::IdleDrop)
                    && loaded.membership.policy().when_idle() != WhenIdle::Drop
            {
                continue;
            }
            for root in &mut loaded.progress.roots {
                let Some(window) = &mut root.window else {
                    continue;
                };
                if !threads.contains(&root.thread_id)
                    || window.selected.is_some()
                    || batch.roots.len() >= MAX_SUBSCRIPTION_NOTICE_ROOTS
                {
                    continue;
                }
                let notice = PendingRootNotice::new(
                    root.thread_id.clone(),
                    loaded.progress.project.clone(),
                    window.from,
                    window.through,
                    window.post_count,
                )
                .map_err(unavailable)?;
                let mut proposed = batch.roots.clone();
                proposed.push(notice.clone());
                if serde_json::to_vec(&proposed).map_err(unavailable)?.len()
                    > maximum_root_notice_bytes
                {
                    if batch.roots.is_empty() {
                        return Err(BoardError::board_unavailable());
                    }
                    break;
                }
                window.selected = Some(SubscriptionWindowSelection {
                    batch_id: batch_id.clone(),
                    through: window.through,
                });
                batch.held |= window.held_since.is_some();
                batch.held_since = batch.held_since.into_iter().chain(window.held_since).min();
                batch.draining |= state == SubscriptionState::Draining;
                batch.roots.push(notice);
                settlement.roots.push(SubscriptionBatchRootSettlement {
                    thread_id: root.thread_id.clone(),
                    window_id: window.window_id.clone(),
                    delivered_through: window.through,
                    subscription_scope: loaded.membership.scope().clone(),
                    subscription_generation: loaded.membership.generation(),
                });
            }
            progress_changes
                .push(SubscriptionDeliveryProgress::new(loaded.progress).map_err(unavailable)?);
        }
        self.automation
            .lock()
            .await
            .put_subscription_progress_batch(&progress_changes)
            .await
            .map_err(unavailable)?;
        Ok((batch, settlement))
    }
    async fn write_batch(
        &mut self,
        reader: &Identity,
        settlement: &SubscriptionBatchSettlement,
        write: WindowWrite,
    ) -> Result<(), BoardError> {
        let projects = self.membership_projects(Some(reader)).await?;
        self.write_project_batch(reader, settlement, write, &projects)
            .await
    }
    pub(in crate::service_identity::subscription_delivery) async fn write_project_batch(
        &mut self,
        reader: &Identity,
        settlement: &SubscriptionBatchSettlement,
        write: WindowWrite,
        projects: &[ProjectId],
    ) -> Result<(), BoardError> {
        for membership in self.memberships_in_projects(Some(reader), projects).await? {
            let mut loaded = self.load(membership).await?;
            for selected in &settlement.roots {
                if selected.subscription_scope != *loaded.membership.scope()
                    || selected.subscription_generation != loaded.membership.generation()
                {
                    continue;
                }
                let Some(root) = loaded
                    .progress
                    .roots
                    .iter_mut()
                    .find(|root| root.thread_id == selected.thread_id)
                else {
                    continue;
                };
                let Some(window) = &mut root.window else {
                    continue;
                };
                if window.window_id != selected.window_id
                    || window
                        .selected
                        .as_ref()
                        .is_none_or(|flight| flight.through != selected.delivered_through)
                {
                    continue;
                }
                match &write {
                    WindowWrite::Settle(outcome) => {
                        if selected.delivered_through.epoch != root.delivered_through.epoch {
                            return Err(invalid_window());
                        }
                        root.delivered_through.position = root
                            .delivered_through
                            .position
                            .max(selected.delivered_through.position);
                        root.window = None;
                        // Later arrivals are recovered by feed rescan; other roots deduplicate by range.
                        loaded.progress.last_outcome = Some(outcome.clone());
                    }
                    WindowWrite::Hold { now, outcome } => {
                        window.selected = None;
                        window.held_since.get_or_insert(*now);
                        window.next_retry_at = None;
                        loaded.progress.last_outcome = Some(outcome.clone());
                    }
                    WindowWrite::Retry { retry_at, outcome } => {
                        window.selected = None;
                        window.held_since = None;
                        window.next_retry_at = Some(*retry_at);
                        window.retry_attempts = window.retry_attempts.saturating_add(1);
                        loaded.progress.last_outcome = Some(outcome.clone());
                    }
                    WindowWrite::Release => window.selected = None,
                }
            }
            // Preserve the validation bound for every root while rescan is requested.
            let max_covered = loaded
                .progress
                .roots
                .iter()
                .flat_map(|root| {
                    [
                        root.delivered_through.position,
                        root.window
                            .as_ref()
                            .map_or(root.delivered_through.position, |window| {
                                window.through.position
                            }),
                    ]
                })
                .max();
            if let Some(max_covered) = max_covered {
                loaded.progress.scan_through.position =
                    loaded.progress.scan_through.position.max(max_covered);
            }
            self.save(&loaded.progress).await?;
        }
        Ok(())
    }
    pub async fn settle_subscription_batch(
        &mut self,
        reader: &Identity,
        batch: &SubscriptionBatchSettlement,
        outcome: SubscriptionDeliveryOutcome,
    ) -> Result<(), BoardError> {
        self.write_batch(reader, batch, WindowWrite::Settle(outcome))
            .await
    }
    pub async fn release_subscription_batch(
        &mut self,
        reader: &Identity,
        batch: &SubscriptionBatchSettlement,
    ) -> Result<(), BoardError> {
        self.write_batch(reader, batch, WindowWrite::Release).await
    }
    pub async fn mark_subscription_batch_held(
        &mut self,
        reader: &Identity,
        batch: &SubscriptionBatchSettlement,
        now: DateTime<Utc>,
        outcome: SubscriptionDeliveryOutcome,
    ) -> Result<(), BoardError> {
        self.write_batch(reader, batch, WindowWrite::Hold { now, outcome })
            .await
    }
    pub async fn mark_subscription_batch_retry(
        &mut self,
        reader: &Identity,
        batch: &SubscriptionBatchSettlement,
        _now: DateTime<Utc>,
        retry_at: DateTime<Utc>,
        outcome: SubscriptionDeliveryOutcome,
    ) -> Result<(), BoardError> {
        self.write_batch(reader, batch, WindowWrite::Retry { retry_at, outcome })
            .await
    }
    pub async fn drop_subscription_roots(
        &mut self,
        reader: &Identity,
        threads: &[ThreadId],
        now: DateTime<Utc>,
    ) -> Result<(), BoardError> {
        let projects = self.membership_projects(Some(reader)).await?;
        self.drop_project_subscription_roots(reader, threads, now, &projects)
            .await
    }
    pub(in crate::service_identity::subscription_delivery) async fn drop_project_subscription_roots(
        &mut self,
        reader: &Identity,
        threads: &[ThreadId],
        now: DateTime<Utc>,
        projects: &[ProjectId],
    ) -> Result<(), BoardError> {
        let (_, batch) = self
            .select_window_notice(
                reader,
                threads,
                now,
                usize::MAX,
                WindowSelectionPurpose::IdleDrop,
                projects,
            )
            .await?;
        self.write_project_batch(
            reader,
            &batch,
            WindowWrite::Settle(SubscriptionDeliveryOutcome::Dropped),
            projects,
        )
        .await
    }
}
enum WindowSelectionPurpose {
    Notice,
    IdleDrop,
}
pub(in crate::service_identity::subscription_delivery) enum WindowWrite {
    Settle(SubscriptionDeliveryOutcome),
    Hold {
        now: DateTime<Utc>,
        outcome: SubscriptionDeliveryOutcome,
    },
    Retry {
        retry_at: DateTime<Utc>,
        outcome: SubscriptionDeliveryOutcome,
    },
    Release,
}
