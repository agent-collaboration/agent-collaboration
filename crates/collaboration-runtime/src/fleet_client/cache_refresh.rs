//! Rebuilds stay private until their frozen snapshot cut catches the live feed.
use super::{FleetClient, FleetClientError};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardReadCursor, BoardReadPage,
    BoardReadScope, FleetFeedRead,
};
use fleet_admission::FleetFeedEntry;
use message_board::{FleetPosition, Record};
use message_board_storage::RouterLocalStorageError;

impl FleetClient {
    pub(super) async fn refresh_cache(&self) -> Result<(), FleetClientError> {
        let mut after = self.cache.checkpoint().await?;
        loop {
            match self.read_feed(after).await? {
                FleetFeedRead::Page {
                    records,
                    through,
                    head,
                    ..
                } => {
                    let entries = decode_feed(after, records, through, head)?;
                    self.cache.apply_feed(after, &entries).await?;
                    if through == head {
                        return Ok(());
                    }
                    after = through;
                }
                FleetFeedRead::CursorExpired { .. } => return self.rebuild_cache().await,
                FleetFeedRead::CursorInvalid => {
                    return Err(FleetClientError::InvalidReply {
                        field: "fleetCursor",
                    });
                }
            }
        }
    }

    async fn rebuild_cache(&self) -> Result<(), FleetClientError> {
        let first = self.read_page(BoardReadScope::FleetSnapshot, None).await?;
        let BoardReadPage::FleetSnapshot {
            records,
            mut next,
            snapshot_at,
            ..
        } = first
        else {
            return Err(FleetClientError::InvalidReply {
                field: "fleetSnapshot",
            });
        };
        if next
            .as_ref()
            .is_some_and(|cursor| cursor.snapshot_at != snapshot_at)
        {
            return Err(FleetClientError::InvalidReply {
                field: "snapshotAt",
            });
        }
        let mut rebuild = self.cache.begin_snapshot_rebuild(snapshot_at).await?;
        rebuild
            .append_snapshot_page(
                snapshot_at,
                &decode_records(records)?,
                next.as_ref().map(|cursor| cursor.after.clone()),
            )
            .await?;
        while let Some(cursor) = next {
            match self
                .read_page(
                    BoardReadScope::FleetSnapshot,
                    Some(BoardReadCursor::FleetSnapshot(cursor)),
                )
                .await?
            {
                BoardReadPage::FleetSnapshot {
                    records,
                    next: continuation,
                    snapshot_at: actual,
                    ..
                } => {
                    rebuild
                        .append_snapshot_page(
                            actual,
                            &decode_records(records)?,
                            continuation.as_ref().map(|cursor| cursor.after.clone()),
                        )
                        .await?;
                    if continuation
                        .as_ref()
                        .is_some_and(|cursor| cursor.snapshot_at != snapshot_at)
                    {
                        return Err(FleetClientError::InvalidReply {
                            field: "snapshotAt",
                        });
                    }
                    next = continuation;
                }
                BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired { floor }) => {
                    return Err(FleetClientError::SnapshotExpired { floor });
                }
                _ => {
                    return Err(FleetClientError::InvalidReply {
                        field: "fleetSnapshot",
                    });
                }
            }
        }
        let mut after = snapshot_at;
        loop {
            match self.read_feed(after).await? {
                FleetFeedRead::Page {
                    records,
                    through,
                    head,
                    ..
                } => {
                    let entries = decode_feed(after, records, through, head)?;
                    rebuild.apply_catchup_page(&entries, through, head).await?;
                    if through == head {
                        return Ok(rebuild.publish().await?);
                    }
                    after = through;
                }
                FleetFeedRead::CursorExpired { floor } => {
                    return Err(FleetClientError::SnapshotExpired { floor });
                }
                FleetFeedRead::CursorInvalid => {
                    return Err(FleetClientError::InvalidReply {
                        field: "fleetCursor",
                    });
                }
            }
        }
    }

    async fn read_feed(&self, after: FleetPosition) -> Result<FleetFeedRead, FleetClientError> {
        match self
            .read_page(
                BoardReadScope::FleetRecords,
                Some(BoardReadCursor::Fleet(after)),
            )
            .await?
        {
            BoardReadPage::FleetFeed(page) => Ok(page),
            _ => Err(FleetClientError::InvalidReply { field: "fleetFeed" }),
        }
    }
    async fn read_page(
        &self,
        scope: BoardReadScope,
        cursor: Option<BoardReadCursor>,
    ) -> Result<BoardReadPage, FleetClientError> {
        match self
            .consumer_request(BoardConsumerRequest::Read { scope, cursor })
            .await?
        {
            BoardConsumerAnswer::Read(page) => Ok(page),
            _ => Err(FleetClientError::InvalidReply { field: "read" }),
        }
    }
}

fn decode_records(records: Vec<BoardPayload>) -> Result<Vec<Record>, FleetClientError> {
    if records.len() > 256 {
        return Err(FleetClientError::InvalidReply { field: "records" });
    }
    records
        .into_iter()
        .map(|record| record.decode().map_err(Into::into))
        .collect()
}
fn decode_feed(
    after: FleetPosition,
    records: Vec<BoardPayload>,
    through: FleetPosition,
    head: FleetPosition,
) -> Result<Vec<FleetFeedEntry>, FleetClientError> {
    if through < after || through > head || (through == after && through != head) {
        return Err(RouterLocalStorageError::PositionGap.into());
    }
    let mut previous = after;
    let mut entries = Vec::new();
    for record in decode_records(records)? {
        let position = record
            .envelope()
            .position
            .fleet_position()
            .ok_or(RouterLocalStorageError::InvalidRecord)?;
        if previous
            .successor()
            .map_err(|_| RouterLocalStorageError::InvalidRecord)?
            != position
        {
            return Err(RouterLocalStorageError::PositionGap.into());
        }
        entries.push(FleetFeedEntry { position, record });
        previous = position;
    }
    if previous != through {
        return Err(RouterLocalStorageError::PositionGap.into());
    }
    Ok(entries)
}
