//! Commit and cold-read the message copy before automation's own migrator drops it.
use crate::{MessageActivityMap, message_copy_sql, message_legacy, message_rows::MessageRows};
use durable_steps::{AtomicStore, CallFailure, CallId, CallTarget, Host, NotSentReason};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, thiserror::Error)]
pub enum MessageUpgradeError {
    #[error("message file operation failed")]
    File(#[from] std::io::Error),
    #[error("legacy message database read failed")]
    Legacy(#[from] rusqlite::Error),
    #[error("message row encoding failed")]
    Encoding(#[from] serde_json::Error),
    #[error("message source is invalid: {field}")]
    InvalidSource { field: &'static str },
    #[error(
        "message scope translation refused: thread_scopes={thread_scopes}, project_scopes={project_scopes}"
    )]
    UnmappedSubscriptionScopes {
        thread_scopes: usize,
        project_scopes: usize,
    },
    #[error(
        "message activity translation refused: missing_thread_ranges={missing_thread_ranges}, empty_thread_ranges={empty_thread_ranges}"
    )]
    UnmappedActivityRanges {
        missing_thread_ranges: usize,
        empty_thread_ranges: usize,
    },
    #[error("message copy differs from its source: {table}")]
    CopyMismatch { table: &'static str },
    #[error("message owner is unavailable")]
    Storage(#[from] interaction_storage::StorageError),
    #[error("message copy transaction failed")]
    Transaction(#[from] durable_steps::StoreError),
    #[error("automation message-drop migration failed")]
    Maintenance(#[from] automation_storage::StorageError),
    #[error("message source task failed")]
    Reader(#[from] tokio::task::JoinError),
    #[error("message source still has an open owner")]
    OpenOwner,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCopyCounts {
    pub pushes: usize,
    pub subscription_progress: usize,
    pub intent_progress: usize,
    pub intent_cursors: usize,
}

/// Per-row SHA-256 uses legacy fields with the named hard-cutover projections.
pub const MESSAGE_COPY_DIGEST_BASIS: &str = "legacy fields; ranges_json=translate(source global activity intervals through board import mapping); origin_router_ref.SubscriptionExpiry.scope=translate(source root/topic through board import mapping); subscription payload=remove retired retry fields; exclude minted progress IDs and retention";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MessageUpgradeReport {
    NoSource,
    AlreadyMoved,
    Verified {
        counts: MessageCopyCounts,
        digest_basis: &'static str,
    },
}

pub struct MessageUpgrade {
    source: PathBuf,
    state: message_legacy::MessageSource,
    mapping: MessageActivityMap,
}

impl MessageUpgrade {
    /// Reads a stopped Router's source without applying any migration to it.
    pub async fn prepare(
        source: &Path,
        mapping: &MessageActivityMap,
    ) -> Result<Self, MessageUpgradeError> {
        Ok(Self {
            source: source.to_owned(),
            state: message_legacy::read(source, mapping).await?,
            mapping: mapping.clone(),
        })
    }

    /// Only the existing InteractionStore opens and writes the destination.
    pub async fn copy_to(
        &self,
        destination: &Path,
    ) -> Result<MessageUpgradeReport, MessageUpgradeError> {
        let rows = match &self.state {
            message_legacy::MessageSource::Absent => return Ok(MessageUpgradeReport::NoSource),
            message_legacy::MessageSource::Moved => return Ok(MessageUpgradeReport::AlreadyMoved),
            message_legacy::MessageSource::Legacy(rows) => rows,
        };
        let owner = Arc::new(InteractionStore::open(destination).await?);
        let native = InteractionAtomicStore::new(owner.clone(), Arc::new(MessageCopyHost));
        let offered = rows.clone();
        let copied = native
            .transaction(move |scope| {
                Box::pin(async move {
                    message_copy_sql::insert(scope, &offered).await?;
                    let stored = message_copy_sql::read(scope).await?;
                    offered
                        .compare(&stored)
                        .map_err(durable_steps::StoreError::backend)?;
                    Ok(())
                })
            })
            .await;
        drop(native);
        let closed = close_owner(owner).await;
        copied?;
        closed?;
        self.verify_copy(destination).await
    }

    /// Reopens the owner, comparing every legacy field and each table's count.
    /// Minted progress IDs and retention are excluded from digests and checked separately.
    pub async fn verify_copy(
        &self,
        destination: &Path,
    ) -> Result<MessageUpgradeReport, MessageUpgradeError> {
        match (
            &self.state,
            message_legacy::read(&self.source, &self.mapping).await?,
        ) {
            (message_legacy::MessageSource::Absent, message_legacy::MessageSource::Absent) => {
                Ok(MessageUpgradeReport::NoSource)
            }
            (message_legacy::MessageSource::Moved, message_legacy::MessageSource::Moved) => {
                Ok(MessageUpgradeReport::AlreadyMoved)
            }
            (
                message_legacy::MessageSource::Legacy(expected),
                message_legacy::MessageSource::Legacy(current),
            ) => {
                expected.compare(&current)?;
                let stored = read_destination(destination).await?;
                current.compare(&stored)?;
                Ok(MessageUpgradeReport::Verified {
                    counts: current.counts(),
                    digest_basis: MESSAGE_COPY_DIGEST_BASIS,
                })
            }
            _ => Err(MessageUpgradeError::InvalidSource {
                field: "source changed during copy",
            }),
        }
    }

    /// Dry runs stop after copy/readback; real upgrades alone invoke this entry.
    pub async fn apply_drop(
        &self,
        destination: &Path,
    ) -> Result<MessageUpgradeReport, MessageUpgradeError> {
        let report = self.verify_copy(destination).await?;
        if matches!(report, MessageUpgradeReport::Verified { .. }) {
            automation_storage::apply_message_drop_migration(&self.source).await?;
        }
        Ok(report)
    }
}

async fn read_destination(path: &Path) -> Result<MessageRows, MessageUpgradeError> {
    let owner = Arc::new(InteractionStore::open(path).await?);
    let native = InteractionAtomicStore::new(owner.clone(), Arc::new(MessageCopyHost));
    let result = native
        .transaction(|scope| Box::pin(message_copy_sql::read(scope)))
        .await;
    drop(native);
    let closed = close_owner(owner).await;
    let rows = result?;
    closed?;
    Ok(rows)
}

async fn close_owner(owner: Arc<InteractionStore>) -> Result<(), MessageUpgradeError> {
    Arc::try_unwrap(owner)
        .map_err(|_| MessageUpgradeError::OpenOwner)?
        .close()
        .await?;
    Ok(())
}

// The named atomic adapter needs a Host, but this offline copy never dispatches or wakes.
pub(crate) struct MessageCopyHost;
impl Clock for MessageCopyHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for MessageCopyHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for MessageCopyHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for MessageCopyHost {
    async fn dispatch<C: CallTarget>(
        &self,
        _: C,
        _: C::Request,
        _: CallId,
    ) -> Result<C::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
