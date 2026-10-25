use super::{ProjectSubscriptionRecord, SubscriptionWindowStore};
use message_board::{
    BoardError, ProjectId, SubscriptionBatch, SubscriptionMode, SubscriptionScope, ThreadId,
    ThreadRef,
};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::{Mutex, oneshot};
use tokio::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionWaitFilter {
    All,
    Roots(Vec<ThreadRef>),
    Project(ProjectId),
}

#[derive(Clone, Debug)]
pub enum SubscriptionWaitResult {
    Notice {
        push_id: collaboration_protocol::PushId,
        line: collaboration_protocol::MessageText,
        batch: SubscriptionBatch,
    },
    Ranges {
        batch: SubscriptionBatch,
    },
}

impl SubscriptionWaitFilter {
    pub(super) fn validate(&self) -> Result<(), BoardError> {
        if let Self::Roots(roots) = self
            && (roots.is_empty()
                || roots
                    .iter()
                    .map(|root| &root.thread)
                    .collect::<HashSet<_>>()
                    .len()
                    != roots.len())
        {
            return Err(BoardError::invalid_field(
                "threads",
                "must contain at least one distinct root",
            ));
        }
        Ok(())
    }

    pub(super) async fn resolve(
        mut self,
        store: &Arc<Mutex<SubscriptionWindowStore>>,
    ) -> Result<Self, BoardError> {
        if let Self::Roots(roots) = &mut self {
            for root in roots {
                root.project = Some(store.lock().await.thread_project(root).await?);
            }
        }
        Ok(self)
    }

    pub(super) fn matches(&self, root: &ThreadId, project: &ProjectId) -> bool {
        match self {
            Self::All => true,
            Self::Roots(roots) => roots.iter().any(|reference| {
                &reference.thread == root && reference.project.as_ref() == Some(project)
            }),
            Self::Project(selected) => selected == project,
        }
    }

    pub(super) fn covered_poll_scopes(
        &self,
        records: &[ProjectSubscriptionRecord],
    ) -> Result<Vec<SubscriptionScope>, BoardError> {
        let mut scopes = Vec::new();
        for row in records
            .iter()
            .filter(|row| row.record.policy().mode() == SubscriptionMode::Poll)
        {
            let record = &row.record;
            let covered = match (self, record.scope()) {
                (Self::All, _) => true,
                (Self::Project(project), _) => project == &row.project,
                (Self::Roots(_), SubscriptionScope::Thread { thread_id }) => {
                    self.matches(thread_id, &row.project)
                }
                (Self::Roots(roots), SubscriptionScope::Project { project_id }) => {
                    roots.iter().any(|root| {
                        root.project.as_ref() == Some(project_id)
                            && !records.iter().any(|subscription| {
                                subscription.project == *project_id
                                    && subscription.record.scope()
                                        == &SubscriptionScope::thread(root.thread.clone())
                            })
                    })
                }
            };
            if covered {
                scopes.push(record.scope().clone());
            }
        }
        if scopes.is_empty() {
            return Err(BoardError::invalid_field(
                "filter",
                "wait requires a poll subscription; use thread subscribe --mode poll",
            ));
        }
        Ok(scopes)
    }
}

pub(super) struct PollWaiter {
    pub filter: SubscriptionWaitFilter,
    pub deadline: Instant,
    pub maximum_root_notice_bytes: usize,
    pub reply: oneshot::Sender<Result<Option<SubscriptionWaitResult>, BoardError>>,
    // Only immutable frozen-filter misses survive a rescan. Temporary gate
    // failures stay eligible for this same request's next existing hint.
    pub poll_filter_mismatches: HashSet<fleet_sync_protocol::ItemId>,
    pub poll_in_flight: Option<fleet_sync_protocol::ItemId>,
}
