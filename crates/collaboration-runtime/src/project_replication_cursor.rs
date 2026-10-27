//! Map the native project owner's acknowledged capture into a feed cursor.
use message_board::{BoardError, EpochNumber, FeedCursor, FeedPosition};

/// A native mapping failure is unavailable state, never a successful zero cursor.
pub async fn project_replication_cursor(
    store: &crate::NativeProjectStore,
    through: fleet_sync_protocol::ReplicationPosition,
    empty_epoch: EpochNumber,
) -> Result<FeedCursor, BoardError> {
    let marks = store.acknowledged_capture(through).await.map_err(|error| {
        tracing::warn!(%error,"native project replication mapping unavailable");
        BoardError::board_unavailable()
    })?;
    Ok(marks.feed.unwrap_or(FeedCursor {
        epoch: empty_epoch,
        position: FeedPosition::ZERO,
    }))
}
