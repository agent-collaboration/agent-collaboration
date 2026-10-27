//! Scan one project into Router-local windows; its read failures never escape to siblings.
use super::{
    BoardError, DateTime, EndReason, FeedContent, FeedPosition, HashSet, Identity,
    MembershipProgress, Placement, ProjectId, Record, SubscriptionMembership, SubscriptionScope,
    SubscriptionWindowId, SubscriptionWindowStore, ThreadId, Utc, WindowStoreError, invalid_window,
    membership_mode_off_or_expired, project_registry_failure, project_storage_failure,
};
use interaction_storage::{SubscriptionRootProgress, SubscriptionWindowProgress};

impl SubscriptionWindowStore {
    pub async fn rescan_missing_subscription_windows(
        &mut self,
        reader: &Identity,
        now: DateTime<Utc>,
    ) -> Result<u64, BoardError> {
        let projects = self.membership_projects(Some(reader)).await?;
        let mut memberships = Vec::new();
        for project in projects {
            memberships.extend(self.project_memberships(Some(reader), &project).await?);
        }
        self.rescan_memberships(reader, now, memberships, &[])
            .await
            .map_err(Into::into)
    }
    pub(in crate::service_identity::subscription_delivery) async fn rescan_project_subscription_windows(
        &mut self,
        reader: &Identity,
        now: DateTime<Utc>,
        projects: &[ProjectId],
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<u64, WindowStoreError> {
        let memberships = self.memberships_in_projects(Some(reader), projects).await?;
        self.rescan_memberships(reader, now, memberships, preventions)
            .await
    }
    pub(super) async fn rescan_memberships(
        &self,
        reader: &Identity,
        now: DateTime<Utc>,
        memberships: Vec<SubscriptionMembership>,
        preventions: &[crate::notification_publisher::GenerationPreventedFollow],
    ) -> Result<u64, WindowStoreError> {
        let thread_overrides: HashSet<_> = memberships
            .iter()
            .filter_map(|membership| membership.scope().thread_id().cloned())
            .collect();
        let mut added = 0_u64;
        for membership in memberships {
            let Some(mut loaded) = self
                .contain_project_failure(self.load_with_preventions(membership, preventions).await)
                .await?
            else {
                continue;
            };
            if preventions
                .iter()
                .any(|fact| fact.belongs_to(&loaded.membership))
            {
                // Rebuild local windows: previous progress cannot settle a newly reopened hole.
                loaded.progress.scan_through = loaded.membership.activated_at();
                loaded.progress.roots.clear();
            }
            if membership_mode_off_or_expired(&loaded.membership, now)
                || loaded
                    .membership
                    .ended()
                    .is_some_and(|ended| ended.reason != EndReason::Resolved)
            {
                // Expiry is derived from the membership; only local pending windows are
                // discarded here. A selected effect retains its settlement evidence.
                for root in &mut loaded.progress.roots {
                    if root
                        .window
                        .as_ref()
                        .is_some_and(|window| window.selected.is_none())
                    {
                        root.window = None;
                    }
                }
                self.save(&loaded.progress).await?;
                continue;
            }
            let Some(scanned) = self
                .contain_project_failure(
                    self.scan_project_window(&mut loaded, reader, &thread_overrides)
                        .await,
                )
                .await?
            else {
                continue;
            };
            added = added.checked_add(scanned).ok_or_else(invalid_window)?;
            self.save(&loaded.progress).await?;
        }
        Ok(added)
    }
    async fn scan_project_window(
        &self,
        loaded: &mut MembershipProgress,
        reader: &Identity,
        thread_overrides: &HashSet<ThreadId>,
    ) -> Result<u64, WindowStoreError> {
        let project = loaded.progress.project.clone();
        let project_error = |error| project_storage_failure(&project, error);
        let store = self
            .registry
            .existing_project(&loaded.progress.project)
            .await
            .map_err(|error| project_registry_failure(&project, error))?
            .ok_or_else(|| WindowStoreError::RetryProject {
                project: project.clone(),
                error: BoardError::board_unavailable(),
                diagnostic: "project disappeared during subscription scan".into(),
            })?;
        let mut read = store.snapshot().await.map_err(project_error)?;
        super::subscription_window_lineage::reconcile_snapshot_progress(
            &mut read,
            &mut loaded.progress,
        )
        .await?;
        let mut added = 0_u64;
        let mut range_closed = HashSet::new();
        let head = read.high_water();
        let upper = loaded
            .membership
            .ended()
            .map_or(head, |ended| ended.position);
        let mut after = loaded.progress.scan_through;
        if let Some(boundary) = loaded
            .progress
            .roots
            .iter()
            .map(|root| root.delivered_through)
            .min_by_key(|cursor| cursor.position)
            && boundary.position < after.position
        {
            after = boundary;
        }
        while after.position < upper.position {
            let count = (upper.position.get() - after.position.get()).min(100);
            let covered_position = FeedPosition::try_from(after.position.get() + count)
                .map_err(|_| invalid_window())?;
            read.validate_feed_range(after.position, covered_position)
                .await
                .map_err(project_error)?;
            let covered_through = read
                .feed_cursor_at(covered_position)
                .await
                .map_err(project_error)?
                .ok_or_else(invalid_window)?;
            let bounds = message_board_storage::FeedPageBounds {
                after: after.position,
                through: covered_through.position,
                direction: message_board_storage::FeedPageDirection::Forward,
                limit: count + 1,
                sweep: None,
            };
            let keys = read
                .post_activity_keys(&bounds, loaded.membership.scope().thread_id())
                .await
                .map_err(project_error)?;
            let threads: std::collections::BTreeSet<_> = keys
                .iter()
                .filter(|key| {
                    key.actor != *reader
                        && match loaded.membership.scope() {
                            SubscriptionScope::Thread { thread_id } => thread_id == &key.thread,
                            SubscriptionScope::Project { .. } => {
                                !thread_overrides.contains(&key.thread)
                            }
                        }
                })
                .map(|key| key.thread.clone())
                .collect();
            let watches = read
                .effective_thread_watch_batch(&threads.into_iter().collect::<Vec<_>>(), reader)
                .await
                .map_err(project_error)?;
            let feed_keys: Vec<_> = keys.iter().map(|key| key.feed.clone()).collect();
            let mut bodies = read
                .feed_entry_batch(&feed_keys)
                .await
                .map_err(project_error)?;
            for key in keys {
                let entry = bodies
                    .remove(&key.feed.cursor.position)
                    .ok_or_else(|| {
                        project_error(message_board_storage::ProjectStorageError::InvalidFeed)
                    })?
                    .decode(false)
                    .map_err(project_error)?;
                let FeedContent::Shown(record) = entry.content else {
                    continue;
                };
                let Record::Post(post) = *record else {
                    continue;
                };
                let thread_id = match &post.placement {
                    Placement::Thread { thread_id } => thread_id.clone(),
                    Placement::Project { .. } => ThreadId::from(post.post_id.clone()),
                };
                if thread_id != key.thread
                    || post.actor != key.actor
                    || post.placement != key.placement
                {
                    return Err(project_error(
                        message_board_storage::ProjectStorageError::InvalidFeed,
                    ));
                }
                let Some(arrival_at) = entry.time.as_router_time() else {
                    // Imported history remains covered, but carries no delivery timing.
                    continue;
                };
                if post.actor == *reader {
                    continue;
                }
                let watched_after = match loaded.membership.scope() {
                    SubscriptionScope::Thread {
                        thread_id: scope_thread,
                    } if scope_thread == &thread_id => watches.get(&thread_id).copied().flatten(),
                    SubscriptionScope::Project { .. } if !thread_overrides.contains(&thread_id) => {
                        watches.get(&thread_id).copied().flatten()
                    }
                    _ => None,
                };
                // Validate the scoped post before applying notification eligibility,
                // including self-posts and posts with a retained opt-out boundary.
                let Some(watched_after) = watched_after else {
                    continue;
                };
                let position = key.feed.cursor;
                if loaded.coverage.iter().any(|range| {
                    range.root() == &thread_id
                        && range.from().position <= position.position
                        && range.through().position >= position.position
                }) {
                    // Keep the first owed hole as this root's current window. A later
                    // scan opens the next hole once this one has its own handoff.
                    if loaded.progress.roots.iter().any(|root| {
                        root.thread_id == thread_id
                            && root
                                .window
                                .as_ref()
                                .is_some_and(|window| window.from.position < position.position)
                    }) {
                        range_closed.insert(thread_id.clone());
                    }
                    continue;
                }
                if range_closed.contains(&thread_id) {
                    continue;
                }
                if position.position <= watched_after.position {
                    continue;
                }
                let root_index = loaded
                    .progress
                    .roots
                    .iter()
                    .position(|root| root.thread_id == thread_id);
                let root = if let Some(index) = root_index {
                    loaded
                        .progress
                        .roots
                        .get_mut(index)
                        .ok_or_else(invalid_window)?
                } else {
                    loaded.progress.roots.push(SubscriptionRootProgress {
                        thread_id: thread_id.clone(),
                        delivered_through: after,
                        window: None,
                    });
                    loaded
                        .progress
                        .roots
                        .last_mut()
                        .ok_or_else(invalid_window)?
                };
                if position.position <= root.delivered_through.position
                    || root
                        .window
                        .as_ref()
                        .is_some_and(|window| position.position <= window.through.position)
                {
                    continue;
                }
                match &mut root.window {
                    Some(window) => {
                        window.through = position;
                        window.post_count = window
                            .post_count
                            .checked_add(1)
                            .ok_or_else(invalid_window)?;
                        window.last_arrival_at = arrival_at;
                    }
                    None => {
                        root.window = Some(SubscriptionWindowProgress {
                            window_id: SubscriptionWindowId::generate(),
                            from: position,
                            through: position,
                            post_count: 1,
                            opened_at: arrival_at,
                            last_arrival_at: arrival_at,
                            selected: None,
                        })
                    }
                }
                added = added.checked_add(1).ok_or_else(invalid_window)?;
            }
            after = covered_through;
        }
        loaded.progress.scan_through = head;
        read.finish().await.map_err(project_error)?;
        Ok(added)
    }
}
