//! Router-local delivery windows and their durable selection/settlement cursors.
use crate::{AutomationStore, StorageError};
use chrono::{DateTime, Utc};
use message_board::{
    BatchId, FeedCursor, ProjectId, RecordId, SubscriptionDeliveryOutcome, SubscriptionGeneration,
    SubscriptionWindowId, ThreadId,
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, SqliteConnection};
use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionWindowSelection {
    pub batch_id: BatchId,
    pub through: FeedCursor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionWindowProgress {
    pub window_id: SubscriptionWindowId,
    pub from: FeedCursor,
    pub through: FeedCursor,
    pub post_count: u64,
    pub opened_at: DateTime<Utc>,
    pub last_arrival_at: DateTime<Utc>,
    pub held_since: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub retry_attempts: u32,
    pub selected: Option<SubscriptionWindowSelection>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionRootProgress {
    pub thread_id: ThreadId,
    pub delivered_through: FeedCursor,
    pub window: Option<SubscriptionWindowProgress>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionDeliveryProgressProps {
    pub project: ProjectId,
    pub membership: RecordId,
    pub generation: SubscriptionGeneration,
    pub activated_at: FeedCursor,
    pub scan_through: FeedCursor,
    pub last_outcome: Option<SubscriptionDeliveryOutcome>,
    pub roots: Vec<SubscriptionRootProgress>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "SubscriptionDeliveryProgressProps",
    into = "SubscriptionDeliveryProgressProps"
)]
pub struct SubscriptionDeliveryProgress {
    project: ProjectId,
    membership: RecordId,
    generation: SubscriptionGeneration,
    activated_at: FeedCursor,
    scan_through: FeedCursor,
    last_outcome: Option<SubscriptionDeliveryOutcome>,
    roots: Vec<SubscriptionRootProgress>,
}

impl SubscriptionDeliveryProgress {
    pub fn new(props: SubscriptionDeliveryProgressProps) -> Result<Self, StorageError> {
        let mut threads = HashSet::new();
        for root in &props.roots {
            if !threads.insert(root.thread_id.clone())
                || root.delivered_through.epoch != props.scan_through.epoch
                || root.delivered_through.position > props.scan_through.position
            {
                return Err(StorageError::InvalidRecord);
            }
            if let Some(window) = &root.window
                && (window.from.epoch != props.scan_through.epoch
                    || window.through.epoch != props.scan_through.epoch
                    || window.from.position <= root.delivered_through.position
                    || window.from.position > window.through.position
                    || window.through.position > props.scan_through.position
                    || window.post_count == 0
                    || window.last_arrival_at < window.opened_at
                    || window.selected.as_ref().is_some_and(|selected| {
                        selected.through.epoch != window.through.epoch
                            || selected.through.position < window.from.position
                            || selected.through.position > window.through.position
                    }))
            {
                return Err(StorageError::InvalidRecord);
            }
        }
        Ok(Self {
            project: props.project,
            membership: props.membership,
            generation: props.generation,
            activated_at: props.activated_at,
            scan_through: props.scan_through,
            last_outcome: props.last_outcome,
            roots: props.roots,
        })
    }
    #[must_use]
    pub fn project(&self) -> &ProjectId {
        &self.project
    }
    #[must_use]
    pub fn membership(&self) -> &RecordId {
        &self.membership
    }
    #[must_use]
    pub fn generation(&self) -> SubscriptionGeneration {
        self.generation
    }
    #[must_use]
    pub fn activated_at(&self) -> FeedCursor {
        self.activated_at
    }
    #[must_use]
    pub fn scan_through(&self) -> FeedCursor {
        self.scan_through
    }
    #[must_use]
    pub fn last_outcome(&self) -> Option<&SubscriptionDeliveryOutcome> {
        self.last_outcome.as_ref()
    }
    #[must_use]
    pub fn roots(&self) -> &[SubscriptionRootProgress] {
        &self.roots
    }
}
impl TryFrom<SubscriptionDeliveryProgressProps> for SubscriptionDeliveryProgress {
    type Error = StorageError;
    fn try_from(props: SubscriptionDeliveryProgressProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<SubscriptionDeliveryProgress> for SubscriptionDeliveryProgressProps {
    fn from(record: SubscriptionDeliveryProgress) -> Self {
        Self {
            project: record.project,
            membership: record.membership,
            generation: record.generation,
            activated_at: record.activated_at,
            scan_through: record.scan_through,
            last_outcome: record.last_outcome,
            roots: record.roots,
        }
    }
}

async fn write_subscription_progress(
    connection: &mut SqliteConnection,
    progress: &SubscriptionDeliveryProgress,
) -> Result<(), StorageError> {
    let project = progress.project.as_str();
    let membership = progress.membership.as_str();
    let activated_epoch = i64::try_from(progress.activated_at.epoch.get())
        .map_err(|_| StorageError::InvalidRecord)?;
    let activated_position = i64::try_from(progress.activated_at.position.get())
        .map_err(|_| StorageError::InvalidRecord)?;
    let payload = serde_json::to_string(progress).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO subscription_delivery_progress(project_id, membership_id, activated_epoch, activated_position, payload) VALUES (?, ?, ?, ?, ?) ON CONFLICT(project_id, membership_id, activated_epoch, activated_position) DO UPDATE SET payload = excluded.payload", project, membership, activated_epoch, activated_position, payload)
            .execute(connection).await?;
    Ok(())
}

impl AutomationStore {
    pub async fn put_subscription_progress(
        &mut self,
        progress: &SubscriptionDeliveryProgress,
    ) -> Result<(), StorageError> {
        self.put_subscription_progress_batch(std::slice::from_ref(progress))
            .await
    }

    /// Selection either owns every persisted window in its handle or persists none.
    pub async fn put_subscription_progress_batch(
        &mut self,
        progress: &[SubscriptionDeliveryProgress],
    ) -> Result<(), StorageError> {
        if progress.is_empty() {
            return Ok(());
        }
        let mut transaction = self.connection.begin().await?;
        for record in progress {
            write_subscription_progress(&mut transaction, record).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
    pub async fn subscription_progress(
        &mut self,
        project: &ProjectId,
        membership: &RecordId,
        activated_at: FeedCursor,
    ) -> Result<Option<SubscriptionDeliveryProgress>, StorageError> {
        let project_key = project.as_str();
        let membership_key = membership.as_str();
        let activated_epoch =
            i64::try_from(activated_at.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let activated_position =
            i64::try_from(activated_at.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        let payload = sqlx::query_scalar!("SELECT payload FROM subscription_delivery_progress WHERE project_id = ? AND membership_id = ? AND activated_epoch = ? AND activated_position = ?", project_key, membership_key, activated_epoch, activated_position)
            .fetch_optional(&mut self.connection).await?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let progress: SubscriptionDeliveryProgress =
            serde_json::from_str(&payload).map_err(|_| StorageError::InvalidRecord)?;
        if progress.project() != project
            || progress.membership() != membership
            || progress.activated_at() != activated_at
        {
            return Err(StorageError::InvalidRecord);
        }
        Ok(Some(progress))
    }
}
