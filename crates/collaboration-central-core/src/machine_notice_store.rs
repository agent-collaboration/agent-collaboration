//! Fleet control streams retain an independent contiguous prefix for each machine.
#[path = "machine_notice_columns.rs"]
mod notice_columns;
use crate::{
    fleet_record_codec::{check_private_id, corrupt, required},
    generated_machine_notices as queries,
};
use board_sync_protocol::{BoardNotice, NoticeRead, PositionedBoardNotice};
use durable_steps::{AtomicStore, JournalScope, StoreError, storage::TransactionScope};
use fleet_protocol::{NoticePosition, PeerId, SyncTime};
use notice_columns::{NoticeColumns, decode_notice};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

const RETENTION_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
struct NoticeState {
    head: u64,
    floor: u64,
}
async fn state<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
) -> Result<Option<NoticeState>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = queries::read_machine_notice_state(scope, peer.to_string())
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("machine_notices.state", "multiple stream heads"));
    }
    let bounds = queries::read_machine_notice_bounds(scope, peer.to_string())
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| corrupt("machine_notices.range", "bounds query returned no row"))?;
    let Some(row) = rows.pop() else {
        if bounds.first_position.is_some() || bounds.last_position.is_some() {
            return Err(corrupt(
                "machine_notices.state",
                "notices exist without their head",
            ));
        }
        return Ok(None);
    };
    if required(
        row.machine_id,
        "machine_control_project_progress.machine_id",
    )? != peer.to_string()
    {
        return Err(corrupt(
            "machine_control_project_progress.machine_id",
            "identity differs",
        ));
    }
    integer(
        row.created_at,
        row.created_class,
        "machine_control_project_progress.created_at",
    )?;
    integer(
        row.updated_at,
        row.updated_class,
        "machine_control_project_progress.updated_at",
    )?;
    let head = position(row.last_issued_position, row.position_class)?;
    let floor = position(row.expired_through, row.floor_class)?;
    if floor > head {
        return Err(corrupt("machine_notices.floor", "floor exceeds head"));
    }
    let first = bound_position(bounds.first_position, bounds.first_class)?;
    let last = bound_position(bounds.last_position, bounds.last_class)?;
    match (first, last) {
        (None, None) if floor == head => {}
        (Some(first), Some(last)) if floor.checked_add(1) == Some(first) && last == head => {}
        _ => {
            return Err(corrupt(
                "machine_notices.range",
                "retained bounds disagree with head and floor",
            ));
        }
    }
    Ok(Some(NoticeState { head, floor }))
}

pub(crate) async fn remember_peer<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if state(scope, peer).await?.is_none() {
        queries::initialize_machine_notice_state(
            scope,
            peer.to_string(),
            now.unix_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    Ok(())
}

async fn prune_peer<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(stored) = state(scope, peer).await? else {
        return Ok(());
    };
    let mut floor = stored.floor;
    let mut previous = stored.floor;
    let mut expired_prefix = true;
    for row in queries::read_machine_notices(scope, peer.to_string(), stored.floor.to_string())
        .await
        .map_err(StoreError::backend)?
    {
        let expires = integer(
            row.expires_at.clone(),
            row.expiry_class.clone(),
            "machine_notices.expires_at",
        )?;
        let notice = decode(row)?;
        if previous.checked_add(1) != Some(notice.position.value())
            || notice.position.value() > stored.head
        {
            return Err(corrupt(
                "machine_notices.range",
                "stored notices are not the exact contiguous range",
            ));
        }
        previous = notice.position.value();
        // Position order is authoritative even when callers sampled time in
        // another order. Validate the suffix, but only prune an expired prefix.
        expired_prefix &= expires <= now.unix_millis();
        if expired_prefix {
            floor = previous;
        }
    }
    if previous != stored.head {
        return Err(corrupt("machine_notices.range", "missing notice"));
    }
    if floor != stored.floor {
        queries::advance_machine_notice_floor(
            scope,
            floor.to_string(),
            now.unix_millis().to_string(),
            peer.to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
        queries::prune_machine_notices(scope, peer.to_string(), floor.to_string())
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

pub(crate) async fn prune<Store: AtomicStore, Driver: Database>(
    store: Arc<Store>,
    now: SyncTime,
) -> Result<(), StoreError>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let peers = store
        .transaction(|scope| {
            Box::pin(async move {
                queries::read_machine_notice_peers(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    let mut first_failure = None;
    for row in peers {
        let result = async {
            let machine_id = required(
                row.machine_id,
                "machine_control_project_progress.machine_id",
            )?;
            check_private_id(&machine_id, "machine_control_project_progress.machine_id")?;
            let peer = machine_id
                .try_into()
                .map_err(|error| corrupt("machine_control_project_progress.machine_id", error))?;
            // A failed machine transaction cannot roll back another stream's
            // floor, deletion or wake. Every row is validated before deletion.
            store
                .transaction(move |scope| {
                    Box::pin(async move {
                        prune_peer(scope, peer, now).await?;
                        if let Some(at) = next_expiry(scope, peer).await? {
                            scope.ensure_wake(at).await?;
                        }
                        Ok(())
                    })
                })
                .await
        }
        .await;
        if let Err(failure) = result {
            eprintln!("Fleet machine notice maintenance failed: {failure}");
            first_failure.get_or_insert(failure);
        }
    }
    first_failure.map_or(Ok(()), Err)
}

pub(crate) async fn append<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    notice_id: String,
    kind: BoardNotice,
    now: SyncTime,
) -> Result<(PositionedBoardNotice, Option<PeerId>), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    prune_peer(scope, peer, now).await?;
    check_private_id(&notice_id, "machine_notices.notice_id")?;
    let stored = if let Some(stored) = state(scope, peer).await? {
        stored
    } else {
        queries::initialize_machine_notice_state(
            scope,
            peer.to_string(),
            now.unix_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
        NoticeState { head: 0, floor: 0 }
    };
    let next = stored
        .head
        .checked_add(1)
        .ok_or_else(|| corrupt("machine_notices.position", "counter exhausted"))?;
    i64::try_from(next).map_err(|error| corrupt("machine_notices.position", error))?;
    let expires_at = now
        .unix_millis()
        .checked_add(RETENTION_MILLIS)
        .ok_or_else(|| corrupt("machine_notices.expiry", "time overflow"))?;
    let columns = NoticeColumns::from_notice(kind);
    queries::append_machine_notice(
        scope,
        notice_id,
        peer.to_string(),
        next.to_string(),
        columns.kind.into(),
        columns.project,
        columns.epoch,
        now.unix_millis().to_string(),
        expires_at.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    queries::advance_machine_notice_head(
        scope,
        next.to_string(),
        now.unix_millis().to_string(),
        peer.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok((
        PositionedBoardNotice {
            position: NoticePosition::new(next),
            kind: columns.notice,
        },
        Some(peer),
    ))
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    after: Option<NoticePosition>,
    now: SyncTime,
) -> Result<NoticeRead, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    prune_peer(scope, peer, now).await?;
    let stored = state(scope, peer)
        .await?
        .unwrap_or(NoticeState { head: 0, floor: 0 });
    let after = after.map_or(stored.floor, |position| position.value());
    if after > stored.head {
        return Ok(NoticeRead::CursorInvalid);
    }
    if after < stored.floor {
        return Ok(NoticeRead::CursorExpired {
            floor: NoticePosition::new(stored.floor),
        });
    }
    let mut notices = Vec::new();
    let mut previous = after;
    for row in queries::read_machine_notices(scope, peer.to_string(), after.to_string())
        .await
        .map_err(StoreError::backend)?
    {
        let notice = decode(row)?;
        if previous.checked_add(1) != Some(notice.position.value())
            || notice.position.value() > stored.head
        {
            return Err(corrupt(
                "machine_notices.range",
                "stored notices are not the exact contiguous range",
            ));
        }
        previous = notice.position.value();
        notices.push(notice);
    }
    if previous != stored.head {
        return Err(corrupt("machine_notices.range", "missing notice"));
    }
    let as_of = chrono::DateTime::from_timestamp_millis(now.unix_millis())
        .ok_or_else(|| corrupt("machine_notices.as_of", "time outside display range"))?;
    Ok(NoticeRead::Page { notices, as_of })
}

pub(crate) async fn next_expiry<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_machine_notice_expiry(scope, peer.to_string())
        .await
        .map_err(StoreError::backend)?
        .and_then(|row| row.expires_at)
        .map(|value| {
            let millis = value
                .parse::<i64>()
                .map_err(|error| corrupt("machine_notices.expiry", error))?;
            if millis.to_string() != value {
                return Err(corrupt("machine_notices.expiry", "noncanonical time"));
            }
            Ok(SyncTime::from_unix_millis(millis))
        })
        .transpose()
}

fn integer(value: Option<String>, class: Option<String>, field: &str) -> Result<i64, StoreError> {
    let value = required(value, field)?;
    let number = value
        .parse::<i64>()
        .map_err(|error| corrupt(field, error))?;
    if required(class, field)? != "integer" || number.to_string() != value {
        return Err(corrupt(field, "noncanonical SQLite integer"));
    }
    Ok(number)
}
fn position(value: Option<String>, class: Option<String>) -> Result<u64, StoreError> {
    u64::try_from(integer(value, class, "machine_notices.position")?)
        .map_err(|error| corrupt("machine_notices.position", error))
}
fn bound_position(value: Option<String>, class: Option<String>) -> Result<Option<u64>, StoreError> {
    if let Some(value) = value {
        return position(Some(value), class).map(Some);
    }
    if required(class, "machine_notices.range")? != "null" {
        return Err(corrupt(
            "machine_notices.range",
            "missing bound is not SQL NULL",
        ));
    }
    Ok(None)
}
fn decode(row: queries::StoredMachineNotice) -> Result<PositionedBoardNotice, StoreError> {
    check_private_id(
        &required(row.machine_id, "machine_control_project_notices.machine_id")?,
        "machine_control_project_notices.machine_id",
    )?;
    check_private_id(
        &required(row.notice_id, "machine_notices.notice_id")?,
        "machine_notices.notice_id",
    )?;
    let position = position(row.position, row.position_class)?;
    if position == 0 {
        return Err(corrupt(
            "machine_notices.position",
            "assigned position is zero",
        ));
    }
    let created = integer(
        row.created_at,
        row.created_class,
        "machine_notices.created_at",
    )?;
    let expires = integer(
        row.expires_at,
        row.expiry_class,
        "machine_notices.expires_at",
    )?;
    if integer(
        row.updated_at,
        row.updated_class,
        "machine_control_project_notices.updated_at",
    )? != created
    {
        return Err(corrupt(
            "machine_control_project_notices.updated_at",
            "an immutable notice has a different update time",
        ));
    }
    if created.checked_add(RETENTION_MILLIS) != Some(expires) {
        return Err(corrupt(
            "machine_notices.retention",
            "invalid expiry interval",
        ));
    }
    Ok(PositionedBoardNotice {
        position: NoticePosition::new(position),
        kind: decode_notice(row.kind, row.project_id, row.epoch)?,
    })
}

#[cfg(test)]
#[path = "machine_notice_store_test.rs"]
mod tests;
