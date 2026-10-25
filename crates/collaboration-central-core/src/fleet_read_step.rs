//! Feed and snapshot boundaries are read with their records in one owned step.
use crate::{
    fleet_feed_store,
    fleet_read_calls::FleetReadRequest,
    fleet_snapshot_read,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::{BoardPayload, BoardReadPage, FleetFeedRead, FleetSnapshotCursor};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use message_board::FleetPosition;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

const PAGE_RECORDS: usize = 256;
pub(crate) struct ReadFleet {
    pub request: FleetReadRequest,
    pub as_of: chrono::DateTime<chrono::Utc>,
    pub installation: message_board::ServiceId,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadFleet
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = BoardReadPage;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            fleet_feed_store::prune(scope, self.as_of).await?;
            let state = fleet_feed_store::state(scope).await?;
            let answer = match self.request {
                FleetReadRequest::Directory { query } => BoardReadPage::Directory {
                    entries: crate::fleet_directory_read::records(scope, &query, self.as_of)
                        .await?
                        .iter()
                        .map(BoardPayload::encode)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(StoreError::backend)?,
                    as_of: self.as_of,
                },
                FleetReadRequest::Feed { after } if after < state.floor => expired(state.floor),
                FleetReadRequest::Feed { after } if after > state.head => invalid(),
                FleetReadRequest::Feed { after } => {
                    let through = FleetPosition::try_from(
                        after
                            .get()
                            .saturating_add(
                                u64::try_from(PAGE_RECORDS).map_err(StoreError::backend)?,
                            )
                            .min(state.head.get()),
                    )
                    .map_err(StoreError::backend)?;
                    let entries = fleet_feed_store::read_range(scope, after, through).await?;
                    crate::fleet_feed_readers::record(
                        scope,
                        &self.installation,
                        after,
                        self.row_ids.next()?,
                    )
                    .await?;
                    let records = entries
                        .into_iter()
                        .map(|entry| {
                            BoardPayload::encode(&entry.record).map_err(StoreError::backend)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    BoardReadPage::FleetFeed(FleetFeedRead::Page {
                        records,
                        through,
                        head: state.head,
                        as_of: self.as_of,
                    })
                }
                FleetReadRequest::Snapshot { cursor } => {
                    let snapshot_at = cursor
                        .as_ref()
                        .map_or(state.head, |cursor| cursor.snapshot_at);
                    if snapshot_at > state.head {
                        invalid()
                    } else if snapshot_at < state.floor {
                        expired(state.floor)
                    } else if cursor.as_ref().is_some_and(|cursor| {
                        crate::fleet_record_codec::kind_tag(cursor.after.kind).is_err()
                    }) {
                        invalid()
                    } else {
                        let (records, next) = fleet_snapshot_read::records(
                            scope,
                            cursor.map(|cursor| cursor.after),
                            PAGE_RECORDS,
                        )
                        .await?;
                        let records = records
                            .iter()
                            .map(BoardPayload::encode)
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(StoreError::backend)?;
                        BoardReadPage::FleetSnapshot {
                            records,
                            next: next.map(|after| FleetSnapshotCursor { snapshot_at, after }),
                            snapshot_at,
                            as_of: self.as_of,
                        }
                    }
                }
            };
            Ok((answer, Vec::new()))
        })
    }
}
fn expired(floor: FleetPosition) -> BoardReadPage {
    BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired { floor })
}
fn invalid() -> BoardReadPage {
    BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid)
}
