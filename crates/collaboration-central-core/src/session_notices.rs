//! One installation's durable numbered hint stream, shared by its two faces.
use crate::copy_storage::{encode_record, query_statement, statement, text};
use crate::{CentralError, RegistryTime, SqlRow, SqlTx, StoreError};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use collaboration_replication_protocol::{
    Notice, NoticeKind, NoticePosition, ReadCursor, ReadPage, SessionAnswer, SessionRefusal,
    SqlValue,
};

const RETENTION: TimeDelta = TimeDelta::days(7);

fn invalid(field: &str) -> CentralError {
    StoreError::Corrupt {
        field: field.into(),
    }
    .into()
}
fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
fn stored_time(row: &SqlRow, column: &str) -> Result<DateTime<Utc>, CentralError> {
    let value = row.text(column)?;
    let parsed: DateTime<Utc> = value.parse().map_err(|_| invalid(column))?;
    if timestamp(parsed) != value {
        return Err(invalid(column));
    }
    Ok(parsed)
}
fn integer(value: u64) -> Result<SqlValue, CentralError> {
    i64::try_from(value)
        .map(SqlValue::Integer)
        .map_err(|_| invalid("notice position limit"))
}
fn position(row: &SqlRow, column: &str) -> Result<u64, CentralError> {
    u64::try_from(row.integer(column)?).map_err(|_| invalid(column))
}
fn head(tx: &mut dyn SqlTx) -> Result<(u64, u64), CentralError> {
    let rows = tx.query(query_statement(
        "SELECT last_position,pruned_through FROM _session_notice_head WHERE singleton=1",
        Vec::new(),
        ["last_position", "pruned_through"],
    ))?;
    let row = rows.first().ok_or_else(|| invalid("notice head"))?;
    let last = position(row, "last_position")?;
    let pruned = position(row, "pruned_through")?;
    if pruned > last {
        return Err(invalid("notice head"));
    }
    Ok((last, pruned))
}
fn decode(row: &SqlRow) -> Result<Notice, CentralError> {
    let position =
        NoticePosition::new(position(row, "position")?).map_err(|_| invalid("notice position"))?;
    let kind = serde_json::from_str(row.text("kind")?).map_err(|_| invalid("notice kind"))?;
    let created = stored_time(row, "created_at")?;
    let expires = stored_time(row, "expires_at")?;
    if created.checked_add_signed(RETENTION) != Some(expires) {
        return Err(invalid("notice retention"));
    }
    Ok(Notice { position, kind })
}

pub(crate) fn prune(tx: &mut dyn SqlTx, now: RegistryTime) -> Result<(), CentralError> {
    let (last, _) = head(tx)?;
    let rows = tx.query(query_statement("SELECT position,kind,created_at,expires_at FROM _session_notices WHERE expires_at<=? ORDER BY position",vec![text(timestamp(now.0))],["position","kind","created_at","expires_at"]))?;
    for row in &rows {
        let notice = decode(row)?;
        if notice.position.value() > last {
            return Err(invalid("notice position"));
        }
        tx.execute(statement("UPDATE _session_notice_head SET pruned_through=max(pruned_through,?) WHERE singleton=1", vec![integer(notice.position.value())?]))?;
    }
    tx.execute(statement(
        "DELETE FROM _session_notices WHERE expires_at<=?",
        vec![text(timestamp(now.0))],
    ))?;
    Ok(())
}

pub(crate) fn append(
    tx: &mut dyn SqlTx,
    kind: &NoticeKind,
    now: RegistryTime,
) -> Result<Notice, CentralError> {
    prune(tx, now)?;
    let (last, _) = head(tx)?;
    let next = last
        .checked_add(1)
        .ok_or_else(|| invalid("notice position limit"))?;
    let expires = now
        .0
        .checked_add_signed(RETENTION)
        .ok_or_else(|| invalid("notice expiry"))?;
    let position = NoticePosition::new(next).map_err(|_| invalid("notice position"))?;
    tx.execute(statement(
        "INSERT INTO _session_notices(position,kind,created_at,expires_at) VALUES(?,?,?,?)",
        vec![
            integer(next)?,
            text(encode_record(kind)?),
            text(timestamp(now.0)),
            text(timestamp(expires)),
        ],
    ))?;
    tx.execute(statement(
        "UPDATE _session_notice_head SET last_position=? WHERE singleton=1",
        vec![integer(next)?],
    ))?;
    Ok(Notice {
        position,
        kind: kind.clone(),
    })
}

pub(crate) fn read(
    tx: &mut dyn SqlTx,
    after: Option<&ReadCursor>,
    now: RegistryTime,
) -> Result<SessionAnswer, CentralError> {
    prune(tx, now)?;
    let (last, floor) = head(tx)?;
    let after = match after {
        None => floor,
        Some(ReadCursor::Notices(position)) if position.value() <= last => {
            if position.value() < floor {
                return Ok(SessionAnswer::Refused(SessionRefusal::CursorExpired));
            }
            position.value()
        }
        Some(_) => {
            return Ok(SessionAnswer::Refused(SessionRefusal::InvalidShape {
                field: "after".into(),
            }));
        }
    };
    let rows = tx.query(query_statement("SELECT position,kind,created_at,expires_at FROM _session_notices WHERE position>? ORDER BY position",vec![integer(after)?],["position","kind","created_at","expires_at"]))?;
    let mut previous = after;
    let mut notices = Vec::new();
    for row in &rows {
        let notice = decode(row)?;
        if previous.checked_add(1) != Some(notice.position.value())
            || notice.position.value() > last
        {
            return Err(invalid("notice sequence"));
        }
        previous = notice.position.value();
        notices.push(notice);
    }
    if previous != last {
        return Err(invalid("notice missing entry"));
    }
    Ok(SessionAnswer::ReadPage(ReadPage::Notices {
        notices,
        as_of: now.0,
    }))
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_session_notices")? {
        return Ok(None);
    }
    let rows = tx.query(query_statement(
        "SELECT expires_at FROM _session_notices ORDER BY expires_at LIMIT 1",
        Vec::new(),
        ["expires_at"],
    ))?;
    rows.first()
        .map(|row| stored_time(row, "expires_at").map(RegistryTime))
        .transpose()
}

pub(crate) fn delivery_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    let notice = next_alarm(tx)?;
    let forward = crate::forward_delivery::next_alarm(tx)?;
    Ok(match (notice, forward) {
        (Some(left), Some(right)) => Some(RegistryTime(left.0.min(right.0))),
        (left, right) => left.or(right),
    })
}
