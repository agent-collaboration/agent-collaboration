//! Decode the selected trail prefix only, retaining exact feed-range coverage.
use super::{admission_error, board_paging::invalid_cursor};
use message_board::*;
use message_board_storage::{FeedPageBounds, FeedPageDirection, ProjectSnapshot, TrailPageFilter};
pub(super) struct TrailReadPrefix {
    pub rows: Vec<WorkTrailRow>,
    pub more: bool,
    pub last_returned: Option<FeedCursor>,
}
pub(super) async fn trail_read_prefix(
    read: &mut ProjectSnapshot,
    bounds: FeedPageBounds,
    filter: TrailPageFilter<'_>,
) -> Result<TrailReadPrefix, BoardError> {
    let limit = usize::try_from(bounds.limit.saturating_sub(1)).map_err(|_| invalid_cursor())?;
    let mut keys = read
        .trail_page_keys(&bounds, filter)
        .await
        .map_err(|error| admission_error(error.into()))?;
    let mut more = keys.len() > limit;
    let (after, through) = if more && matches!(bounds.direction, FeedPageDirection::Backward) {
        let last = keys
            .last()
            .ok_or_else(BoardError::board_unavailable)?
            .cursor;
        (
            FeedPosition::try_from(last.position.get().saturating_sub(1))
                .map_err(|_| invalid_cursor())?,
            bounds.through,
        )
    } else {
        (bounds.after, bounds.through)
    };
    read.validate_feed_range(after, through)
        .await
        .map_err(|error| admission_error(error.into()))?;
    keys.truncate(limit);
    let mut entries = read
        .feed_entry_batch(&keys)
        .await
        .map_err(|error| admission_error(error.into()))?;
    let mut rows = Vec::new();
    let mut bytes = 2_usize;
    let mut last_returned = None;
    for key in keys {
        let entry = entries
            .remove(&key.cursor.position)
            .ok_or_else(BoardError::board_unavailable)?
            .decode(filter.include_redacted)
            .map_err(|error| admission_error(error.into()))?;
        let FeedContent::Shown(record) = entry.content else {
            return Err(BoardError::board_unavailable());
        };
        let Record::WorkTrailRow(row) = *record else {
            return Err(BoardError::board_unavailable());
        };
        let size = serde_json::to_vec(&row)
            .map_err(|_| BoardError::board_unavailable())?
            .len();
        if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
            if rows.is_empty() {
                return Err(BoardError::board_unavailable());
            }
            more = true;
            break;
        }
        bytes = bytes.saturating_add(size).saturating_add(1);
        last_returned = Some(key.cursor);
        rows.push(row);
    }
    Ok(TrailReadPrefix {
        rows,
        more,
        last_returned,
    })
}
