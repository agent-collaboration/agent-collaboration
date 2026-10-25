//! Contiguous Fleet feed publication and central-time retention in the owned scope.

use crate::{
    fleet_record_codec::{
        FleetRecordHeader, check_private_id, corrupt, decode_json, required, validate_record,
    },
    generated_fleet_feed as queries,
};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetFeedEntry;
use message_board::FleetPosition;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct FeedState {
    pub head: FleetPosition,
    pub floor: FleetPosition,
}

pub(crate) async fn high_water<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<FleetPosition, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    Ok(state(scope).await?.head)
}

pub(crate) async fn state<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<FeedState, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = queries::read_fleet_feed_state(scope)
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("feed_state", "multiple Fleet feed heads"));
    }
    let Some(row) = rows.pop() else {
        if !queries::read_fleet_feed_page(scope, "0".into(), "1".into())
            .await
            .map_err(StoreError::backend)?
            .is_empty()
        {
            return Err(corrupt("feed_state", "feed exists without its head"));
        }
        return Ok(FeedState {
            head: FleetPosition::ZERO,
            floor: FleetPosition::ZERO,
        });
    };
    check_private_id(&required(row.state_id, "feed_state.id")?, "feed_state.id")?;
    if required(row.name, "feed_state.name")? != "fleet" {
        return Err(corrupt("feed_state.name", "unknown feed state"));
    }
    let head = stored_position(row.last_position, row.position_class)?;
    let floor = stored_position(row.retained_through, row.floor_class)?;
    if floor > head {
        return Err(corrupt(
            "feed_state.floor",
            "retention floor is beyond feed head",
        ));
    }
    Ok(FeedState { head, floor })
}

/// Read exactly the bounded committed range chosen by the caller's head/floor check.
/// A missing row is corruption, never a shorter successful prefix.
pub(crate) async fn read_range<Scope: TransactionScope>(
    scope: &mut Scope,
    after: FleetPosition,
    through: FleetPosition,
) -> Result<Vec<FleetFeedEntry>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if through < after {
        return Err(corrupt("feed.range", "range end precedes its start"));
    }
    i64::try_from(after.get()).map_err(StoreError::backend)?;
    i64::try_from(through.get()).map_err(StoreError::backend)?;
    let count = through.get().saturating_sub(after.get());
    if count == 0 {
        return Ok(Vec::new());
    }
    let rows = queries::read_fleet_feed_page(scope, after.get().to_string(), count.to_string())
        .await
        .map_err(StoreError::backend)?;
    let mut previous = after;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let (entry, _) = decode_entry(row)?;
        if previous.successor().map_err(StoreError::backend)? != entry.position
            || entry.position > through
        {
            return Err(corrupt(
                "feed.range",
                "stored feed is not the exact contiguous range",
            ));
        }
        previous = entry.position;
        entries.push(entry);
    }
    if previous != through {
        return Err(corrupt(
            "feed.range",
            "stored feed is missing a committed entry",
        ));
    }
    Ok(entries)
}

pub(crate) async fn append<Scope: TransactionScope>(
    scope: &mut Scope,
    entries: &[FleetFeedEntry],
    now: DateTime<Utc>,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    prune(scope, now).await?;
    let state = state(scope).await?;
    if entries.is_empty() {
        return Ok(());
    }
    if state.head == FleetPosition::ZERO {
        queries::initialize_fleet_feed_state(scope, row_ids.next()?.to_string())
            .await
            .map_err(StoreError::backend)?;
    }
    let mut previous = state.head;
    for entry in entries {
        let header = FleetRecordHeader::for_record(&entry.record)?;
        if previous.successor().map_err(StoreError::backend)? != entry.position
            || entry.record.envelope().position.fleet_position() != Some(entry.position)
        {
            return Err(corrupt(
                "feed.position",
                "feed entry is not the next committed position",
            ));
        }
        i64::try_from(entry.position.get()).map_err(StoreError::backend)?;
        queries::append_fleet_feed(
            scope,
            row_ids.next()?.to_string(),
            serde_json::to_string(entry).map_err(StoreError::backend)?,
            header.router_time(),
            header.time_basis(),
            timestamp(now),
        )
        .await
        .map_err(StoreError::backend)?;
        previous = entry.position;
    }
    queries::advance_fleet_feed_head(scope, previous.get().to_string())
        .await
        .map_err(StoreError::backend)
}

/// Retention is a clock rule even when no new records are appended.
pub(crate) async fn prune<Scope: TransactionScope>(
    scope: &mut Scope,
    now: DateTime<Utc>,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let state = state(scope).await?;
    let cutoff = timestamp(
        now.checked_sub_signed(TimeDelta::days(30))
            .ok_or_else(|| corrupt("feed.retention", "time underflow"))?,
    );
    let expired = queries::read_expired_fleet_feed(scope, cutoff.clone())
        .await
        .map_err(StoreError::backend)?;
    let mut floor = state.floor;
    for row in expired {
        let (entry, _) = decode_entry(row)?;
        if floor.successor().map_err(StoreError::backend)? != entry.position
            || entry.position > state.head
        {
            return Err(corrupt(
                "feed.retention",
                "expired entries do not form a contiguous prefix",
            ));
        }
        floor = entry.position;
    }
    if floor != state.floor {
        queries::advance_fleet_feed_floor(scope, floor.get().to_string())
            .await
            .map_err(StoreError::backend)?;
        queries::prune_fleet_feed(scope, cutoff)
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

fn decode_entry(
    row: queries::StoredFleetFeedEntry,
) -> Result<(FleetFeedEntry, DateTime<Utc>), StoreError> {
    check_private_id(&required(row.entry_id, "feed.entry_id")?, "feed.entry_id")?;
    let position = stored_position(row.position, row.position_class)?;
    let entry: FleetFeedEntry =
        decode_json(&required(row.payload, "feed.payload")?, "feed.payload")?;
    validate_record(&entry.record)?;
    if entry.position != position
        || entry.record.envelope().position.fleet_position() != Some(position)
        || required(row.record_id, "feed.record_id")? != entry.record.reference().id.as_str()
    {
        return Err(corrupt(
            "feed.position",
            "entry headers disagree with payload",
        ));
    }
    FleetRecordHeader::decode(
        row.router_time,
        row.time_basis,
        row.origin_installation,
        row.origin_machine,
    )?
    .check_record(&entry.record)?;
    let encoded_time = required(row.as_of, "feed.as_of")?;
    let as_of: DateTime<Utc> = encoded_time.parse().map_err(StoreError::backend)?;
    if timestamp(as_of) != encoded_time {
        return Err(corrupt("feed.as_of", "noncanonical retention timestamp"));
    }
    Ok((entry, as_of))
}

fn stored_position(
    value: Option<String>,
    class: Option<String>,
) -> Result<FleetPosition, StoreError> {
    let value = required(value, "feed.position")?;
    let position: u64 = value
        .parse()
        .map_err(|error| corrupt("feed.position", error))?;
    if required(class, "feed.position_class")? != "integer"
        || position.to_string() != value
        || i64::try_from(position).is_err()
    {
        return Err(corrupt(
            "feed.position",
            "position is not a nonnegative SQLite integer",
        ));
    }
    FleetPosition::try_from(position).map_err(|error| corrupt("feed.position", error))
}
fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
