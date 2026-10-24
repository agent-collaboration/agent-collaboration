//! Decode board metadata from the captured row image, never from the live head.
use crate::{ProjectStorageError, project_record_codec::decode_record_time};
use fleet_sync_protocol::{ReplicatedTransaction, RowChange, SqlValue};
use fleet_sync_writer::CapturedTransaction;
use message_board::{
    EpochNumber, FeedCursor, FeedPosition, RecordId, RecordRef, RecordTime, StatusName,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedFeedSummary {
    pub cursor: FeedCursor,
    pub status: Option<StatusName>,
    pub time: RecordTime,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapturedProjectChanges {
    pub first_feed_position: Option<FeedPosition>,
    pub latest_feed: Option<CapturedFeedSummary>,
    pub max_handoff_seq: Option<u64>,
    pub max_expansion_seq: Option<u64>,
    pub settled_through: Option<u64>,
}

impl CapturedProjectChanges {
    pub fn decode(captured: &CapturedTransaction<'_>) -> Result<Self, ProjectStorageError> {
        let mut changes = Self::default();
        let ReplicatedTransaction::Rows { rows, .. } = captured.transaction else {
            return Ok(changes);
        };
        for row in rows {
            let RowChange::Upsert { table, key, values } = row else {
                continue;
            };
            match table.as_str() {
                "project_feed" => {
                    validate_row(key, values, 13)?;
                    let [
                        _,
                        epoch,
                        position,
                        record_kind,
                        record_id,
                        _,
                        _,
                        _,
                        _,
                        router_time,
                        time_basis,
                        _,
                        project_status,
                    ] = values.as_slice()
                    else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    let epoch = EpochNumber::try_from(counter(epoch)?)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?;
                    let position = FeedPosition::try_from(counter(position)?)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?;
                    if position == FeedPosition::ZERO {
                        return Err(ProjectStorageError::InvalidFeed);
                    }
                    let reference = RecordRef {
                        kind: serde_json::from_value(serde_json::Value::String(
                            text(record_kind)?.to_owned(),
                        ))
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                        id: RecordId::try_from(text(record_id)?.to_owned())
                            .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    };
                    let time = decode_record_time(
                        optional_text(router_time)?.map(str::to_owned),
                        text(time_basis)?,
                        &reference,
                    )?;
                    let status = optional_text(project_status)?
                        .map(|value| {
                            let status = StatusName::try_from(value.to_owned())
                                .map_err(|_| ProjectStorageError::InvalidFeed)?;
                            if status.as_str() != value {
                                return Err(ProjectStorageError::InvalidFeed);
                            }
                            Ok(status)
                        })
                        .transpose()?;
                    changes.first_feed_position = Some(
                        changes
                            .first_feed_position
                            .map_or(position, |first| first.min(position)),
                    );
                    if changes
                        .latest_feed
                        .as_ref()
                        .is_none_or(|latest| position > latest.cursor.position)
                    {
                        changes.latest_feed = Some(CapturedFeedSummary {
                            cursor: FeedCursor { epoch, position },
                            status,
                            time,
                        });
                    }
                }
                "mail_handoffs" => {
                    validate_row(key, values, 3)?;
                    let [_, sequence, _] = values.as_slice() else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    let sequence = counter(sequence)?;
                    if sequence == 0 {
                        return Err(ProjectStorageError::InvalidFeed);
                    }
                    changes.max_handoff_seq = Some(
                        changes
                            .max_handoff_seq
                            .map_or(sequence, |previous| previous.max(sequence)),
                    );
                }
                "mail_cancel_expansions" => {
                    validate_row(key, values, 5)?;
                    let [id, sequence, group, target, envelope] = values.as_slice() else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    let sequence = counter(sequence)?;
                    let envelope: fleet_sync_protocol::DeliveryEnvelope<
                        board_sync_protocol::BoardDeliveryTypes,
                    > = serde_json::from_str(text(envelope)?)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?;
                    let fleet_sync_protocol::DeliveryEnvelope::Cancel(cancel) = envelope else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    if sequence == 0
                        || !fleet_sync_protocol::canonical_uuid_v7(text(group)?)
                        || !fleet_sync_protocol::canonical_uuid_v7(text(target)?)
                        || cancel.id().as_uuid().to_string() != text(id)?
                        || cancel.target().as_uuid().to_string() != text(target)?
                    {
                        return Err(ProjectStorageError::InvalidFeed);
                    }
                    changes.max_expansion_seq = Some(
                        changes
                            .max_expansion_seq
                            .map_or(sequence, |previous| previous.max(sequence)),
                    );
                }
                "handoff_frontier" => {
                    validate_row(key, values, 2)?;
                    let [_, frontier] = values.as_slice() else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    let frontier = counter(frontier)?;
                    changes.settled_through = Some(
                        changes
                            .settled_through
                            .map_or(frontier, |previous| previous.max(frontier)),
                    );
                }
                _ => {}
            }
        }
        Ok(changes)
    }
}

fn validate_row(
    key: &[SqlValue],
    values: &[SqlValue],
    columns: usize,
) -> Result<(), ProjectStorageError> {
    if values.len() != columns
        || key.first() != values.first()
        || key.len() != 1
        || !fleet_sync_protocol::canonical_uuid_v7(text(
            values.first().ok_or(ProjectStorageError::InvalidFeed)?,
        )?)
    {
        return Err(ProjectStorageError::InvalidFeed);
    }
    Ok(())
}

fn counter(value: &SqlValue) -> Result<u64, ProjectStorageError> {
    match value {
        SqlValue::Integer(value) => {
            u64::try_from(*value).map_err(|_| ProjectStorageError::InvalidFeed)
        }
        _ => Err(ProjectStorageError::InvalidFeed),
    }
}

fn text(value: &SqlValue) -> Result<&str, ProjectStorageError> {
    match value {
        SqlValue::Text(value) => Ok(value),
        _ => Err(ProjectStorageError::InvalidFeed),
    }
}

fn optional_text(value: &SqlValue) -> Result<Option<&str>, ProjectStorageError> {
    match value {
        SqlValue::Null => Ok(None),
        _ => text(value).map(Some),
    }
}
