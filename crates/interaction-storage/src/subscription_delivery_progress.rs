//! Router-local delivery windows and their durable selection/settlement cursors.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};
use message_board::{
    BatchId, FeedCursor, ProjectId, RecordId, SubscriptionDeliveryOutcome, SubscriptionGeneration,
    SubscriptionWindowId, ThreadId,
};
use serde::{Deserialize, Serialize};
use sqlx::Connection;
use sqlx_turso::TursoConnection;
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

impl SubscriptionDeliveryProgressProps {
    /// Every local cursor needs the project file's lineage check before it is used.
    #[must_use]
    pub fn cursors(&self) -> Vec<FeedCursor> {
        let mut cursors = vec![self.activated_at, self.scan_through];
        for root in &self.roots {
            cursors.push(root.delivered_through);
            if let Some(window) = &root.window {
                cursors.extend([window.from, window.through]);
                if let Some(selected) = &window.selected {
                    cursors.push(selected.through);
                }
            }
        }
        cursors
    }
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
        let mut cursors = props.cursors();
        cursors.sort_by_key(|cursor| (cursor.position, cursor.epoch));
        if cursors
            .iter()
            .zip(cursors.iter().skip(1))
            .any(|(earlier_cursor, later_cursor)| {
                earlier_cursor.position < later_cursor.position
                    && earlier_cursor.epoch > later_cursor.epoch
            })
        {
            return Err(StorageError::InvalidRecord);
        }
        let mut threads = HashSet::new();
        for root in &props.roots {
            if !threads.insert(root.thread_id.clone())
                || root.delivered_through.position > props.scan_through.position
            {
                return Err(StorageError::InvalidRecord);
            }
            if let Some(window) = &root.window
                && (window.from.position <= root.delivered_through.position
                    || window.from.position > window.through.position
                    || window.through.position > props.scan_through.position
                    || window.post_count == 0
                    || window.last_arrival_at < window.opened_at
                    || window.selected.as_ref().is_some_and(|selected| {
                        selected.through.position < window.from.position
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
    connection: &mut TursoConnection,
    progress: &SubscriptionDeliveryProgress,
) -> Result<(), StorageError> {
    let project = progress.project.as_str();
    let membership = progress.membership.as_str();
    let activated_epoch = i64::try_from(progress.activated_at.epoch.get())
        .map_err(|_| StorageError::InvalidRecord)?;
    let activated_position = i64::try_from(progress.activated_at.position.get())
        .map_err(|_| StorageError::InvalidRecord)?;
    let payload = serde_json::to_string(progress).map_err(|_| StorageError::InvalidRecord)?;
    let progress_id = uuid::Uuid::now_v7().to_string();
    let row = sqlx_turso::query!(r#"INSERT INTO subscription_delivery_progress(progress_id, project_id, membership_id, activated_epoch, activated_position, payload) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(project_id, membership_id, activated_epoch, activated_position) DO UPDATE SET payload = excluded.payload RETURNING progress_id AS "progress_id!: String""#, progress_id, project, membership, activated_epoch, activated_position, payload)
        .fetch_one(connection).await?;
    crate::worker_progress_id::decode_worker_progress_id(row.progress_id)?;
    Ok(())
}

impl InteractionStore {
    pub async fn put_subscription_progress(
        &self,
        progress: &SubscriptionDeliveryProgress,
    ) -> Result<(), StorageError> {
        self.put_subscription_progress_batch(std::slice::from_ref(progress))
            .await
    }

    /// Selection either owns every persisted window in its handle or persists none.
    pub async fn put_subscription_progress_batch(
        &self,
        progress: &[SubscriptionDeliveryProgress],
    ) -> Result<(), StorageError> {
        if progress.is_empty() {
            return Ok(());
        }
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let written = async {
            for record in progress {
                write_subscription_progress(&mut transaction, record).await?;
            }
            Ok::<_, StorageError>(())
        }
        .await;
        if let Err(error) = written {
            transaction.rollback().await?;
            return Err(error);
        }
        transaction.commit().await?;
        Ok(())
    }
    pub async fn subscription_progress(
        &self,
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
        let mut connection = self.writer.lock().await;
        let row = sqlx_turso::query!(r#"SELECT progress_id AS "progress_id!: String", payload AS "payload!: String" FROM subscription_delivery_progress WHERE project_id = ? AND membership_id = ? AND activated_epoch = ? AND activated_position = ?"#, project_key, membership_key, activated_epoch, activated_position)
            .fetch_optional(&mut *connection).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        crate::worker_progress_id::decode_worker_progress_id(row.progress_id)?;
        let progress: SubscriptionDeliveryProgress =
            serde_json::from_str(&row.payload).map_err(|_| StorageError::InvalidRecord)?;
        if progress.project() != project
            || progress.membership() != membership
            || progress.activated_at() != activated_at
        {
            return Err(StorageError::InvalidRecord);
        }
        Ok(Some(progress))
    }
}

#[cfg(test)]
#[path = "subscription_progress_epoch_tests.rs"]
mod epoch_tests;
