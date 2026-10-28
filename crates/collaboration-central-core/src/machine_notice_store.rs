//! The connection object's authoritative notice stream and verified peer snapshot.
use crate::{
    fleet_record_codec::{check_private_id, corrupt, decode_json, required},
    generated_machine_notices as queries,
};
use board_sync_protocol::{BoardNotice, NoticeRead, PositionedBoardNotice};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_protocol::{NoticePosition, PeerId, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

const RETENTION_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
struct NoticeState {
    head: u64,
    floor: u64,
    peer: Option<PeerId>,
}
async fn state<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<NoticeState>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = queries::read_machine_notice_state(scope)
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("machine_notices.state", "multiple stream heads"));
    }
    let bounds = queries::read_machine_notice_bounds(scope)
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
    check_private_id(
        &required(row.state_id, "machine_notices.state_id")?,
        "machine_notices.state_id",
    )?;
    if required(row.name, "machine_notices.name")? != "machine" {
        return Err(corrupt("machine_notices.name", "unknown stream"));
    }
    let head = position(row.last_position, row.position_class)?;
    let floor = position(row.pruned_through, row.floor_class)?;
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
    let peer = row
        .peer_id
        .map(|value| {
            let peer = PeerId::try_from(value.clone())
                .map_err(|error| corrupt("machine_notices.peer", error))?;
            if peer.to_string() != value {
                return Err(corrupt("machine_notices.peer", "noncanonical peer"));
            }
            Ok(peer)
        })
        .transpose()?;
    Ok(Some(NoticeState { head, floor, peer }))
}

pub(crate) async fn remember_peer<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    state_id: String,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let stored = state(scope).await?;
    if stored
        .as_ref()
        .and_then(|state| state.peer)
        .is_some_and(|stored| stored != peer)
    {
        return Err(StoreError::Aborted {
            reason: "verified admission peer contradicts the machine's retained identity".into(),
        });
    }
    if stored.is_none() {
        check_private_id(&state_id, "machine_notices.state_id")?;
        queries::initialize_machine_notice_state(scope, state_id)
            .await
            .map_err(StoreError::backend)?;
    }
    queries::remember_machine_notice_peer(scope, peer.to_string())
        .await
        .map_err(StoreError::backend)
}

pub(crate) async fn prune<Scope: TransactionScope>(
    scope: &mut Scope,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(stored) = state(scope).await? else {
        return Ok(());
    };
    let mut floor = stored.floor;
    for row in queries::read_expired_machine_notices(scope, now.unix_millis().to_string())
        .await
        .map_err(StoreError::backend)?
    {
        let notice = decode(row)?;
        if floor.checked_add(1) != Some(notice.position.value())
            || notice.position.value() > stored.head
        {
            return Err(corrupt(
                "machine_notices.retention",
                "expired notices are not a contiguous prefix",
            ));
        }
        floor = notice.position.value();
    }
    if floor != stored.floor {
        queries::advance_machine_notice_floor(scope, floor.to_string())
            .await
            .map_err(StoreError::backend)?;
        queries::prune_machine_notices(scope, floor.to_string())
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

pub(crate) async fn append<Scope: TransactionScope>(
    scope: &mut Scope,
    notice_id: String,
    kind: BoardNotice,
    now: SyncTime,
) -> Result<(PositionedBoardNotice, Option<PeerId>), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    prune(scope, now).await?;
    check_private_id(&notice_id, "machine_notices.notice_id")?;
    let stored = if let Some(stored) = state(scope).await? {
        stored
    } else {
        queries::initialize_machine_notice_state(scope, notice_id.clone())
            .await
            .map_err(StoreError::backend)?;
        NoticeState {
            head: 0,
            floor: 0,
            peer: None,
        }
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
    queries::append_machine_notice(
        scope,
        notice_id,
        next.to_string(),
        serde_json::to_string(&kind).map_err(StoreError::backend)?,
        now.unix_millis().to_string(),
        expires_at.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    queries::advance_machine_notice_head(scope, next.to_string())
        .await
        .map_err(StoreError::backend)?;
    Ok((
        PositionedBoardNotice {
            position: NoticePosition::new(next),
            kind,
        },
        stored.peer,
    ))
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    after: Option<NoticePosition>,
    now: SyncTime,
) -> Result<NoticeRead, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    prune(scope, now).await?;
    let stored = state(scope).await?.unwrap_or(NoticeState {
        head: 0,
        floor: 0,
        peer: None,
    });
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
    for row in queries::read_machine_notices(scope, after.to_string())
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
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_machine_notice_expiry(scope)
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
    if created.checked_add(RETENTION_MILLIS) != Some(expires) {
        return Err(corrupt(
            "machine_notices.retention",
            "invalid expiry interval",
        ));
    }
    Ok(PositionedBoardNotice {
        position: NoticePosition::new(position),
        kind: decode_json(
            &required(row.kind, "machine_notices.kind")?,
            "machine_notices.kind",
        )?,
    })
}
