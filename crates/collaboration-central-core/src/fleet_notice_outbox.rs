//! Fleet commits hints with their causes; only its later alarm dispatches them.
use crate::copy_storage::{encode_record, query_statement, statement, text};
use crate::fleet_storage::corrupt;
use crate::*;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use collaboration_replication_protocol::{
    BoardId, DirectoryLease, FleetPosition, InstallationId, NoticeKind, ProjectId, SqlValue,
};
use fleet_admission::{DirectorySide, FleetFeedEntry, LeaseMetadata};
use message_board::{BoardState, Record};
use std::collections::BTreeSet;

const RETRY: TimeDelta = TimeDelta::seconds(5);
fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
fn stored_time(value: &str) -> Result<DateTime<Utc>, CentralError> {
    let parsed: DateTime<Utc> = value.parse().map_err(|_| corrupt("notice outbox time"))?;
    if timestamp(parsed) != value {
        return Err(corrupt("notice outbox time"));
    }
    Ok(parsed)
}

pub(crate) fn owe(
    tx: &mut dyn SqlTx,
    installation: &InstallationId,
    kind: &NoticeKind,
    now: RegistryTime,
) -> Result<(), CentralError> {
    tx.execute(statement("INSERT OR IGNORE INTO notice_outbox(installation,kind,owed_at,next_attempt_at) VALUES(?,?,?,?)",vec![text(installation.as_str()),text(encode_record(kind)?),text(timestamp(now.0)),text(timestamp(now.0))]))?;
    Ok(())
}

pub(crate) fn feed_appended(
    tx: &mut dyn SqlTx,
    entries: &[FleetFeedEntry],
    now: RegistryTime,
) -> Result<(), CentralError> {
    let Some(last) = entries.last() else {
        return Ok(());
    };
    let readers = tx.query(query_statement(
        "SELECT installation,last_read_position FROM feed_readers",
        Vec::new(),
        ["installation", "last_read_position"],
    ))?;
    for row in &readers {
        let installation = InstallationId::new(row.text("installation")?)
            .map_err(|_| corrupt("feed reader installation"))?;
        let through = u64::try_from(row.integer("last_read_position")?)
            .map_err(|_| corrupt("feed reader position"))?;
        if through > last.position.get() {
            return Err(corrupt("feed reader position"));
        }
        if through < last.position.get() {
            owe(
                tx,
                &installation,
                &NoticeKind::FleetFeedAdvanced {
                    through: FleetPosition::new(last.position.get()),
                },
                now,
            )?;
        }
    }
    for entry in entries {
        let Record::Board(board) = &entry.record else {
            continue;
        };
        if board.state != BoardState::Archiving {
            continue;
        }
        let projects = tx.query(query_statement(
            "SELECT project_id FROM fleet_directory WHERE board_id=?",
            vec![text(board.board_id.as_str())],
            ["project_id"],
        ))?;
        let mut holders = BTreeSet::new();
        for row in &projects {
            let project = ProjectId::new(row.text("project_id")?)
                .map_err(|_| corrupt("archiving project"))?;
            if let DirectorySide::Known(LeaseMetadata {
                lease: DirectoryLease::Held { holder, .. },
                ..
            }) = crate::directory_receiver::lease_metadata(tx, &project)?
            {
                holders.insert(holder);
            }
        }
        for holder in holders {
            owe(
                tx,
                &holder,
                &NoticeKind::BoardArchiving {
                    board: BoardId::new(board.board_id.as_str())
                        .map_err(|_| corrupt("archiving board"))?,
                },
                now,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn alarm(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<FleetEffect>, CentralError> {
    let rows=tx.query(query_statement("SELECT installation,kind,owed_at,next_attempt_at,call_id FROM notice_outbox WHERE next_attempt_at<=? ORDER BY owed_at,installation,kind LIMIT 8",vec![text(timestamp(now.0))],["installation","kind","owed_at","next_attempt_at","call_id"]))?;
    let mut effects = Vec::new();
    for row in rows.iter().take(ids.call_ids.len()) {
        let installation = InstallationId::new(row.text("installation")?)
            .map_err(|_| corrupt("notice outbox installation"))?;
        let kind: NoticeKind =
            serde_json::from_str(row.text("kind")?).map_err(|_| corrupt("notice outbox kind"))?;
        let owed = stored_time(row.text("owed_at")?)?;
        let due = stored_time(row.text("next_attempt_at")?)?;
        if due < owed {
            return Err(corrupt("notice outbox deadline"));
        }
        match row.value("call_id")? {
            SqlValue::Null => {}
            SqlValue::Text(id) => {
                ObjectCallId::new(id).map_err(|_| corrupt("notice outbox call ID"))?;
            }
            _ => return Err(corrupt("notice outbox call ID")),
        }
        let id = ids
            .call_ids
            .pop()
            .ok_or_else(|| CentralError::InvalidInput {
                message: "shell supplied no notice call ID".into(),
            })?;
        let retry = now
            .0
            .checked_add_signed(RETRY)
            .ok_or_else(|| corrupt("notice outbox deadline"))?;
        tx.execute(statement(
            "UPDATE notice_outbox SET call_id=?,next_attempt_at=? WHERE installation=? AND kind=?",
            vec![
                text(id.as_str()),
                text(timestamp(retry)),
                text(installation.as_str()),
                text(row.text("kind")?),
            ],
        ))?;
        effects.push(FleetEffect::Dispatch(FleetDispatch::Delivery(Dispatch {
            call_id: id,
            to: installation,
            call: DeliveryCall::DeliverNotice(kind),
            carry: None,
        })));
    }
    if !rows.is_empty() && effects.is_empty() {
        return Err(CentralError::InvalidInput {
            message: "shell supplied no notice call ID".into(),
        });
    }
    Ok(effects)
}

pub(crate) fn acknowledged(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    result: &FleetReply,
) -> Result<(), CentralError> {
    if !matches!(
        result,
        FleetReply::Delivery(DeliveryResult::NoticeDelivered)
    ) {
        return Err(CentralError::ReplyContract);
    }
    tx.execute(statement(
        "DELETE FROM notice_outbox WHERE call_id=?",
        vec![text(id.as_str())],
    ))?;
    Ok(())
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    let outbox = if crate::registry_dispatch::has_table(tx, "notice_outbox")? {
        let rows = tx.query(query_statement(
            "SELECT next_attempt_at FROM notice_outbox ORDER BY next_attempt_at LIMIT 1",
            Vec::new(),
            ["next_attempt_at"],
        ))?;
        rows.first()
            .map(|row| stored_time(row.text("next_attempt_at")?).map(RegistryTime))
            .transpose()?
    } else {
        None
    };
    let feed = crate::fleet_feed::next_alarm(tx)?;
    Ok(match (outbox, feed) {
        (Some(left), Some(right)) => Some(RegistryTime(left.0.min(right.0))),
        (left, right) => left.or(right),
    })
}
