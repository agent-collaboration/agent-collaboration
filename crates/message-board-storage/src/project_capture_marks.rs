//! Holder-local mappings commit with the sender's capture assignment.
use crate::{CapturedProjectChanges, ProjectStorageError, ProjectStore};
use durable_steps::{Host, storage::TransactionScope};
use fleet_sync_protocol::ReplicationPosition;
use fleet_sync_receiver::{SnapshotInstall, SnapshotInstallState};
use fleet_sync_writer::{
    CapturedTransaction, MaintenanceTransaction, ReplicatedSchema, WriteTransaction,
    WriterJournalScope,
};
use message_board::{EpochNumber, FeedCursor, FeedPosition, HandoffSeq};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AcknowledgedProjectChanges {
    pub feed: Option<FeedCursor>,
    pub max_handoff_seq: Option<u64>,
    pub max_expansion_seq: Option<u64>,
    pub settled_through: Option<u64>,
}

impl<TSchema: ReplicatedSchema> ProjectStore<TSchema> {
    /// The installed file's original retirement floor; a file without a seed has none.
    pub async fn verified_seed_frontier(&self) -> Result<Option<HandoffSeq>, ProjectStorageError> {
        let Some(receipt) = self.database.installed_snapshot().await? else {
            return Ok(None);
        };
        let mut read = self.database.reader().begin_read().await?;
        let result = read_verified_seed_frontier(&mut read, &receipt).await;
        read.close().await?;
        result.map(Some)
    }

    pub async fn acknowledged_capture(
        &self,
        through: ReplicationPosition,
    ) -> Result<AcknowledgedProjectChanges, ProjectStorageError> {
        self.reader().acknowledged_capture(through).await
    }
}

impl crate::ProjectSnapshot {
    pub async fn acknowledged_capture(
        &mut self,
        through: ReplicationPosition,
    ) -> Result<AcknowledgedProjectChanges, ProjectStorageError> {
        acknowledged_capture(&mut self.transaction, through).await
    }
}

pub(crate) async fn acknowledged_capture<TScope>(
    scope: &mut TScope,
    through: ReplicationPosition,
) -> Result<AcknowledgedProjectChanges, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let epoch =
        i64::try_from(through.epoch.value()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let position =
        i64::try_from(through.sequence.value()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let marks = sqlx_turso::query!(
            r#"SELECT MAX(last_feed_position) AS "feed?: i64",MAX(max_handoff_seq) AS "handoff?: i64",MAX(max_expansion_seq) AS "expansion?: i64",MAX(settled_through) AS "frontier?: i64" FROM capture_feed_marks WHERE epoch<? OR (epoch=? AND captured_position<=?)"#,
            epoch, epoch, position,
        ).fetch_one(&mut *scope).await?;
    let feed = if let Some(position) = marks.feed {
        let entries = sqlx_turso::query!(
            r#"SELECT epoch AS "epoch!: i64" FROM project_feed WHERE position=? LIMIT 2"#,
            position,
        )
        .fetch_all(&mut *scope)
        .await?;
        let [entry] = entries.as_slice() else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        Some(FeedCursor {
            epoch: EpochNumber::try_from(
                unsigned(Some(entry.epoch))?.ok_or(ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
            position: FeedPosition::try_from(
                unsigned(Some(position))?.ok_or(ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
        })
    } else {
        None
    };
    let result = AcknowledgedProjectChanges {
        feed,
        max_handoff_seq: unsigned(marks.handoff)?,
        max_expansion_seq: unsigned(marks.expansion)?,
        settled_through: unsigned(marks.frontier)?,
    };
    Ok(result)
}

fn unsigned(value: Option<i64>) -> Result<Option<u64>, ProjectStorageError> {
    value
        .map(|value| u64::try_from(value).map_err(|_| ProjectStorageError::InvalidFeed))
        .transpose()
}

pub(crate) async fn require_snapshot_capture(
    transaction: &mut WriteTransaction,
    receipt: &SnapshotInstallState,
) -> Result<(), ProjectStorageError> {
    read_verified_seed_frontier(transaction, receipt)
        .await
        .map(|_| ())
}

async fn read_verified_seed_frontier<TScope>(
    transaction: &mut TScope,
    receipt: &SnapshotInstallState,
) -> Result<HandoffSeq, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    if !receipt.verified {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let epoch = i64::try_from(receipt.install.position.epoch.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let position = i64::try_from(receipt.install.position.sequence.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let existing=sqlx_turso::query!(
        r#"SELECT snapshot_identity AS "identity?: String",settled_through AS "frontier?: i64" FROM capture_feed_marks WHERE epoch=? AND captured_position=?"#,
        epoch,position,
    ).fetch_optional(&mut *transaction).await?.ok_or(ProjectStorageError::SnapshotBaselinePending)?;
    let saved: SnapshotInstall = serde_json::from_str(
        existing
            .identity
            .as_deref()
            .ok_or(ProjectStorageError::InvalidFeed)?,
    )?;
    if saved != receipt.install {
        return Err(ProjectStorageError::InvalidFeed);
    }
    HandoffSeq::try_from(unsigned(existing.frontier)?.unwrap_or(0))
        .map_err(|_| ProjectStorageError::InvalidFeed)
}

/// The install owner calls this before exposing the published file to admission.
/// The Host holds its install/admission gate and supplies `installed_snapshot()`'s
/// verified native receipt; this transaction compares it with the saved identity.
pub async fn record_snapshot_capture<H: Host>(
    scope: &mut WriterJournalScope<'_, H>,
    receipt: &SnapshotInstallState,
) -> Result<(), ProjectStorageError> {
    if !receipt.verified {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let install = &receipt.install;
    let scope = scope.executor();
    let epoch = i64::try_from(install.position.epoch.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let position = i64::try_from(install.position.sequence.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let existing = sqlx_turso::query!(
        r#"SELECT snapshot_identity AS "identity?: String" FROM capture_feed_marks WHERE epoch=? AND captured_position=?"#,
        epoch, position,
    ).fetch_optional(&mut *scope).await?;
    if let Some(existing) = existing {
        let saved: SnapshotInstall = serde_json::from_str(
            existing
                .identity
                .as_deref()
                .ok_or(ProjectStorageError::InvalidFeed)?,
        )?;
        if saved != *install {
            return Err(ProjectStorageError::InvalidFeed);
        }
        return Ok(());
    }
    let baseline = sqlx_turso::query!(
        r#"SELECT (SELECT MAX(position) FROM project_feed) AS "feed?: i64",(SELECT MAX(handoff_seq) FROM mail_handoffs) AS "handoff?: i64",(SELECT MAX(expansion_seq) FROM mail_cancel_expansions) AS "expansion?: i64",(SELECT MAX(settled_through) FROM handoff_frontier) AS "frontier?: i64""#,
    ).fetch_one(&mut *scope).await?;
    // Decode before writing: corrupt seed metadata never becomes an acknowledgement.
    unsigned(baseline.feed)?;
    unsigned(baseline.handoff)?;
    unsigned(baseline.expansion)?;
    unsigned(baseline.frontier)?;
    let identity = serde_json::to_string(install)?;
    sqlx_turso::query!(
        "INSERT INTO capture_feed_marks(epoch,captured_position,last_feed_position,max_handoff_seq,max_expansion_seq,settled_through,snapshot_identity) VALUES(?,?,?,?,?,?,?)",
        epoch, position, baseline.feed, baseline.handoff, baseline.expansion, baseline.frontier, identity,
    ).execute(&mut *scope).await?;
    Ok(())
}

pub async fn record_project_capture(
    scope: &mut MaintenanceTransaction<'_>,
    captured: &CapturedTransaction<'_>,
) -> Result<(), ProjectStorageError> {
    let changes = CapturedProjectChanges::decode(captured)?;
    let epoch = i64::try_from(captured.position.epoch.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let position = i64::try_from(captured.position.sequence.value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let first = changes
        .first_feed_position
        .map(|value| i64::try_from(value.get()))
        .transpose()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let last = changes
        .latest_feed
        .as_ref()
        .map(|value| i64::try_from(value.cursor.position.get()))
        .transpose()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let handoff = changes
        .max_handoff_seq
        .map(i64::try_from)
        .transpose()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let frontier = changes
        .settled_through
        .map(i64::try_from)
        .transpose()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let expansion = changes
        .max_expansion_seq
        .map(i64::try_from)
        .transpose()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    sqlx_turso::query!(
        "INSERT INTO capture_feed_marks(epoch,captured_position,first_feed_position,last_feed_position,max_handoff_seq,max_expansion_seq,settled_through) VALUES(?,?,?,?,?,?,?) ON CONFLICT(epoch,captured_position) DO NOTHING",
        epoch, position, first, last, handoff, expansion, frontier,
    ).execute(&mut *scope).await?;
    let stored = sqlx_turso::query!(
        r#"SELECT first_feed_position AS "first?: i64",last_feed_position AS "last?: i64",max_handoff_seq AS "handoff?: i64",max_expansion_seq AS "expansion?: i64",settled_through AS "frontier?: i64" FROM capture_feed_marks WHERE epoch=? AND captured_position=?"#,
        epoch, position,
    ).fetch_one(&mut *scope).await?;
    if (
        stored.first,
        stored.last,
        stored.handoff,
        stored.expansion,
        stored.frontier,
    ) != (first, last, handoff, expansion, frontier)
    {
        return Err(ProjectStorageError::InvalidFeed);
    }
    Ok(())
}
