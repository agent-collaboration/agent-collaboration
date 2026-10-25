//! Readback goes to the actual Fleet owner over the fixture's existing session.
use crate::{FixtureResult, FleetTestRuntime};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardReadCursor, BoardReadPage, BoardReadScope,
    FleetFeedRead,
};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use message_board::{FleetPosition, Record};

impl FleetTestRuntime {
    async fn read_fleet(
        &self,
        scope: BoardReadScope,
        cursor: Option<BoardReadCursor>,
    ) -> FixtureResult<BoardReadPage> {
        match self
            .requests
            .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
                scope,
                cursor,
            }))
            .await
            .map_err(|error| format!("fixture Fleet read did not answer: {error:?}"))?
        {
            OperationAnswer::Consumer(BoardConsumerAnswer::Read(page)) => Ok(page),
            _ => Err("fixture Fleet read returned another operation family".into()),
        }
    }

    pub async fn fleet_snapshot(&self) -> FixtureResult<(Vec<Record>, FleetPosition)> {
        let mut cursor = None;
        let mut cut = None;
        let mut records = Vec::new();
        loop {
            let BoardReadPage::FleetSnapshot {
                records: page,
                next,
                snapshot_at,
                ..
            } = self
                .read_fleet(BoardReadScope::FleetSnapshot, cursor.clone())
                .await?
            else {
                return Err("fixture Fleet snapshot returned another read family".into());
            };
            if cut.is_some_and(|cut| cut != snapshot_at) {
                return Err("fixture Fleet snapshot changed its cut".into());
            }
            cut = Some(snapshot_at);
            records.extend(
                page.into_iter()
                    .map(|payload| payload.decode::<Record>())
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let Some(next) = next else {
                return Ok((records, snapshot_at));
            };
            let next = Some(BoardReadCursor::FleetSnapshot(next));
            if cursor == next {
                return Err("fixture Fleet snapshot cursor did not advance".into());
            }
            cursor = next;
        }
    }

    pub async fn fleet_feed(&self, after: FleetPosition) -> FixtureResult<FleetFeedRead> {
        match self
            .read_fleet(
                BoardReadScope::FleetRecords,
                Some(BoardReadCursor::Fleet(after)),
            )
            .await?
        {
            BoardReadPage::FleetFeed(page) => Ok(page),
            _ => Err("fixture Fleet feed returned another read family".into()),
        }
    }
}
