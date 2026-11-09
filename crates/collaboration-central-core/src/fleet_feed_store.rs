//! Contiguous Fleet feed publication and central-time retention in the owned scope.

use crate::fleet_feed_header::timestamp;
use crate::{
    fleet_record_codec::{check_private_id, corrupt, required},
    generated_fleet_feed as queries,
};
use chrono::{DateTime, TimeDelta, Utc};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetFeedEntry;
use message_board::FleetPosition;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct FeedState {
    /// The one state row's UUIDv7, absent until step 0 creates it.
    pub id: Option<String>,
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
            id: None,
            head: FleetPosition::ZERO,
            floor: FleetPosition::ZERO,
        });
    };
    let id = required(row.state_id, "feed_state.id")?;
    check_private_id(&id, "feed_state.id")?;
    crate::fleet_record_codec::check_millis(
        row.created_at,
        row.created_class,
        "feed_state.created_at",
    )?;
    crate::fleet_record_codec::check_millis(
        row.updated_at,
        row.updated_class,
        "feed_state.updated_at",
    )?;
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
    Ok(FeedState {
        id: Some(id),
        head,
        floor,
    })
}

/// The Fleet object's failure subject: its feed state row's UUIDv7.
pub(crate) async fn state_subject<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<durable_steps::Uuid, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let id = state(scope)
        .await?
        .id
        .ok_or_else(|| corrupt("feed_state", "the Fleet feed state row is missing"))?;
    durable_steps::Uuid::parse_str(&id).map_err(|error| corrupt("feed_state.id", error))
}

/// Step 0: create the one feed state row if it doesn't exist yet. Its id is
/// also the Fleet object's subject for failures with no row of their own.
/// An existing row is not decoded here: a corrupt row must fail only the work
/// that reads it, never the object's open (#failures).
pub(crate) async fn ensure_state<Scope: TransactionScope>(
    scope: &mut Scope,
    id: durable_steps::Uuid,
    now: DateTime<Utc>,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if !queries::read_fleet_feed_state(scope)
        .await
        .map_err(StoreError::backend)?
        .is_empty()
    {
        return Ok(());
    }
    queries::initialize_fleet_feed_state(scope, id.to_string(), now.timestamp_millis().to_string())
        .await
        .map_err(StoreError::backend)
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
    let mut children = crate::fleet_feed_rows::read_range(
        scope,
        after.get().to_string(),
        count.to_string(),
        &rows,
    )
    .await?;
    let mut previous = after;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let (entry, _) = decode_entry(row, &mut children)?;
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
    children.finish()?;
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
    // Append neither prunes nor creates the state row: retention is its own due
    // source, and step 0 creates the row (#pipeline).
    let state = state(scope).await?;
    if entries.is_empty() {
        return Ok(());
    }
    if state.id.is_none() {
        return Err(corrupt(
            "feed_state",
            "the Fleet feed state row is missing; step 0 creates it",
        ));
    }
    let mut previous = state.head;
    for entry in entries {
        if previous.successor().map_err(StoreError::backend)? != entry.position
            || entry.record.envelope().position.fleet_position() != Some(entry.position)
        {
            return Err(corrupt(
                "feed.position",
                "feed entry is not the next committed position",
            ));
        }
        i64::try_from(entry.position.get()).map_err(StoreError::backend)?;
        crate::fleet_feed_rows::append(scope, entry, row_ids.next()?, now, row_ids).await?;
        previous = entry.position;
    }
    queries::advance_fleet_feed_head(
        scope,
        previous.get().to_string(),
        now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// The retention cutoff: rows at or before it have passed the 30-day rule.
fn retention_cutoff(now: DateTime<Utc>) -> Result<String, StoreError> {
    Ok(timestamp(
        now.checked_sub_signed(TimeDelta::days(30))
            .ok_or_else(|| corrupt("feed.retention", "time underflow"))?,
    ))
}

/// The floor the feed has at `now` (exclusive): the stored floor advanced over
/// the contiguous prefix that has passed retention, whether or not retention
/// has deleted it yet. Reads use this, so they never depend on a prune.
pub(crate) async fn effective_floor<Scope: TransactionScope>(
    scope: &mut Scope,
    state: &FeedState,
    now: DateTime<Utc>,
) -> Result<FleetPosition, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let cutoff = retention_cutoff(now)?;
    let expired = queries::read_expired_fleet_feed(scope, cutoff.clone())
        .await
        .map_err(StoreError::backend)?;
    if expired.is_empty() {
        return Ok(state.floor);
    }
    let mut children = crate::fleet_feed_rows::read_expired(scope, cutoff, &expired).await?;
    let mut floor = state.floor;
    for row in expired {
        let (entry, _) = decode_entry(row, &mut children)?;
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
    children.finish()?;
    Ok(floor)
}

/// Retention, run only by its due source: delete what passed the 30-day rule
/// and advance the stored floor in the same transaction.
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
    let floor = effective_floor(scope, &state, now).await?;
    let cutoff = retention_cutoff(now)?;
    if floor != state.floor {
        queries::advance_fleet_feed_floor(
            scope,
            floor.get().to_string(),
            now.timestamp_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
        crate::generated_fleet_feed_lists::prune_feed_repositories(scope, cutoff.clone())
            .await
            .map_err(StoreError::backend)?;
        crate::generated_fleet_feed_lists::prune_feed_adr_files(scope, cutoff.clone())
            .await
            .map_err(StoreError::backend)?;
        queries::prune_fleet_feed(scope, cutoff)
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

fn decode_entry(
    mut row: queries::StoredFleetFeedEntry,
    children: &mut crate::fleet_feed_rows::FeedLists,
) -> Result<(FleetFeedEntry, DateTime<Utc>), StoreError> {
    let header = crate::fleet_feed_header::decode(&mut row)?;
    let kind = crate::fleet_feed_snapshot::kind(&mut row)?;
    let columns = crate::fleet_feed_snapshot::SnapshotColumns::decode(&mut row)?;
    let (repositories, files) = children.take(&header.entry_id);
    let record = columns.record(
        kind,
        header.envelope,
        &header.entry_id,
        repositories,
        files,
        &mut children.requests,
    )?;
    Ok((
        FleetFeedEntry {
            position: header.position,
            record,
        },
        header.as_of,
    ))
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

/// When the oldest retained feed row passes the 30-day retention rule, if any row is retained.
pub(crate) async fn next_retention_at<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<runtime_primitives::SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let state = state(scope).await?;
    // While retention fails, its attempt row says when to try again.
    if let Some(id) = &state.id {
        let subject =
            durable_steps::Uuid::parse_str(id).map_err(|error| corrupt("feed_state.id", error))?;
        match crate::work_attempts::schedule(
            scope,
            subject,
            crate::work_attempts::AttemptKind::FeedRetention,
        )
        .await?
        {
            crate::work_attempts::SubjectSchedule::Unscheduled => {}
            crate::work_attempts::SubjectSchedule::RetryAt(at) => return Ok(Some(at)),
            crate::work_attempts::SubjectSchedule::NotDue => return Ok(None),
        }
    }
    let Some(mut oldest) =
        queries::read_fleet_feed_page(scope, state.floor.get().to_string(), "1".into())
            .await
            .map_err(StoreError::backend)?
            .into_iter()
            .next()
    else {
        return Ok(None);
    };
    let header = crate::fleet_feed_header::decode(&mut oldest)?;
    // prune() removes rows with as_of <= now - 30 days, so a row becomes due exactly then.
    let due = header
        .as_of
        .checked_add_signed(TimeDelta::days(30))
        .ok_or_else(|| corrupt("feed.retention", "time overflow"))?;
    Ok(Some(runtime_primitives::SyncTime::from_unix_millis(
        due.timestamp_millis(),
    )))
}
