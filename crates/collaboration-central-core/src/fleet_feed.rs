//! Fleet's transactional append sequence and central-time retention.
use crate::copy_storage::{encode_record, query_statement, statement, text};
use crate::fleet_storage::{checkpoint, corrupt};
use crate::{CentralError, RegistryTime, SqlRow, SqlTx};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use collaboration_replication_protocol::{
    FleetPayload, FleetPosition, InstallationId, ReadCursor, ReadPage, SessionAnswer,
    SessionRefusal, SqlValue,
};
use fleet_admission::FleetFeedEntry;
use message_board::{Record, RecordPosition};

const RETENTION: TimeDelta = TimeDelta::days(30);

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn stored_time(value: &str) -> Result<DateTime<Utc>, CentralError> {
    let time: DateTime<Utc> = value.parse().map_err(|_| corrupt("fleet feed as_of"))?;
    if timestamp(time) != value {
        return Err(corrupt("fleet feed as_of"));
    }
    Ok(time)
}

fn position_value(position: u64) -> Result<SqlValue, CentralError> {
    i64::try_from(position)
        .map(SqlValue::Integer)
        .map_err(|_| corrupt("fleet feed position"))
}

fn stored_position(row: &SqlRow, column: &str) -> Result<u64, CentralError> {
    u64::try_from(row.integer(column)?).map_err(|_| corrupt("fleet feed position"))
}

pub(crate) fn append(
    tx: &mut dyn SqlTx,
    entries: &[FleetFeedEntry],
    now: RegistryTime,
) -> Result<(), CentralError> {
    let mut previous = checkpoint(tx)?;
    for entry in entries {
        if previous
            .successor()
            .map_err(|_| corrupt("fleet feed position"))?
            != entry.position
            || entry.record.envelope().position != RecordPosition::Fleet(entry.position)
        {
            return Err(corrupt("fleet feed position"));
        }
        tx.execute(statement(
            "INSERT INTO fleet_feed(position,entry,as_of) VALUES(?,?,?)",
            vec![
                position_value(entry.position.get())?,
                text(encode_record(entry)?),
                text(timestamp(now.0)),
            ],
        ))?;
        tx.execute(statement(
            "UPDATE fleet_checkpoint SET last_position=? WHERE singleton=1",
            vec![position_value(entry.position.get())?],
        ))?;
        previous = entry.position;
    }
    crate::fleet_notice_outbox::feed_appended(tx, entries, now)?;
    Ok(())
}

pub(crate) fn prune(tx: &mut dyn SqlTx, now: RegistryTime) -> Result<(), CentralError> {
    let cutoff = timestamp(
        now.0
            .checked_sub_signed(RETENTION)
            .ok_or_else(|| corrupt("fleet retention time"))?,
    );
    let expired = tx.query(query_statement(
        "SELECT position,as_of FROM fleet_feed WHERE as_of<=? ORDER BY position",
        vec![text(&cutoff)],
        ["position", "as_of"],
    ))?;
    for row in &expired {
        stored_time(row.text("as_of")?)?;
        let position = stored_position(row, "position")?;
        tx.execute(statement(
            "UPDATE fleet_feed_retention_floor SET through_position=max(through_position,?) WHERE singleton=1",
            vec![position_value(position)?],
        ))?;
    }
    tx.execute(statement(
        "DELETE FROM fleet_feed WHERE as_of<=?",
        vec![text(cutoff)],
    ))?;
    Ok(())
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "fleet_feed")? {
        return Ok(None);
    }
    let rows = tx.query(query_statement(
        "SELECT as_of FROM fleet_feed ORDER BY as_of LIMIT 1",
        Vec::new(),
        ["as_of"],
    ))?;
    rows.first()
        .map(|row| {
            Ok(RegistryTime(
                stored_time(row.text("as_of")?)?
                    .checked_add_signed(RETENTION)
                    .ok_or_else(|| corrupt("fleet retention time"))?,
            ))
        })
        .transpose()
}

pub(crate) fn read(
    tx: &mut dyn SqlTx,
    installation: &InstallationId,
    after: Option<&ReadCursor>,
    now: RegistryTime,
) -> Result<SessionAnswer, CentralError> {
    let head = checkpoint(tx)?.get();
    let invalid = || {
        SessionAnswer::Refused(SessionRefusal::InvalidShape {
            field: "after".into(),
        })
    };
    let records = match after {
        None => crate::fleet_storage::all_records(tx)?,
        Some(ReadCursor::Fleet(after)) => {
            let after = after.clone().value();
            if after > head {
                return Ok(invalid());
            }
            let floor = tx.query(query_statement(
                "SELECT through_position FROM fleet_feed_retention_floor WHERE singleton=1",
                Vec::new(),
                ["through_position"],
            ))?;
            let floor = stored_position(
                floor
                    .first()
                    .ok_or_else(|| corrupt("fleet feed retention floor"))?,
                "through_position",
            )?;
            if floor > head {
                return Err(corrupt("fleet feed retention floor"));
            }
            if after < floor {
                return Ok(SessionAnswer::Refused(SessionRefusal::CursorExpired));
            }
            let mut previous = after;
            let records = tx.query(query_statement(
                "SELECT position,entry,as_of FROM fleet_feed WHERE position>? ORDER BY position",
                vec![position_value(after)?],
                ["position", "entry", "as_of"],
            ))?
            .iter()
            .map(|row| {
                stored_time(row.text("as_of")?)?;
                let entry: FleetFeedEntry = serde_json::from_str(row.text("entry")?)
                    .map_err(|_| corrupt("fleet feed entry"))?;
                if previous.checked_add(1) != Some(entry.position.get())
                    || entry.position.get() != stored_position(row, "position")?
                    || entry.record.envelope().position != RecordPosition::Fleet(entry.position)
                {
                    return Err(corrupt("fleet feed entry position"));
                }
                previous = entry.position.get();
                crate::fleet_storage::decode_record(&encode_record(&entry.record)?)
            })
            .collect::<Result<Vec<Record>, CentralError>>()?;
            if previous != head {
                return Err(corrupt("fleet feed missing entry"));
            }
            records
        }
        Some(ReadCursor::Notices(_) | ReadCursor::Snapshot(_)) => return Ok(invalid()),
    };
    tx.execute(statement(
        "INSERT INTO feed_readers(installation,last_read_position) VALUES(?,?) ON CONFLICT(installation) DO UPDATE SET last_read_position=excluded.last_read_position",
        vec![text(installation.as_str()), position_value(head)?],
    ))?;
    let records = records
        .iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(FleetPayload::new)
                .map_err(|error| CentralError::Encoding {
                    message: error.to_string(),
                })
        })
        .collect::<Result<_, _>>()?;
    Ok(SessionAnswer::ReadPage(ReadPage::FleetRecords {
        records,
        position: FleetPosition::new(head),
        as_of: now.0,
    }))
}
