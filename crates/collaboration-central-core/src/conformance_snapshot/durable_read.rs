//! The S8 durable read: every row of each source table, decoded or refused.

use super::{
    AttemptRecord, DurableSnapshot, FeedRecord, NoticeProgressRecord, OutboxOwnerRecord,
    OwnersRecord, SnapshotRefusal, StatusOwedRecord,
};
use crate::generated_snapshot as queries;
use durable_steps::{Uuid, storage::TransactionScope};
use fleet_protocol::{ConnectionId, PeerId, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

const ATTEMPTS: &str = "central_work_attempts";
const PROGRESS: &str = "machine_control_project_progress";

/// One row's columns, decoded against the table and key they came from.
struct Columns<'row> {
    table: &'static str,
    key: &'row Option<String>,
}

impl Columns<'_> {
    fn refuse(&self, field: &'static str) -> SnapshotRefusal {
        SnapshotRefusal::CorruptRow {
            table: self.table,
            key: self.key.clone().unwrap_or_default(),
            field,
        }
    }

    fn text(&self, field: &'static str, value: Option<String>) -> Result<String, SnapshotRefusal> {
        value.ok_or_else(|| self.refuse(field))
    }

    fn uuid(&self, field: &'static str, value: &Option<String>) -> Result<Uuid, SnapshotRefusal> {
        self.optional_uuid(field, value)?
            .ok_or_else(|| self.refuse(field))
    }

    fn optional_uuid(
        &self,
        field: &'static str,
        value: &Option<String>,
    ) -> Result<Option<Uuid>, SnapshotRefusal> {
        value
            .as_deref()
            .map(|text| Uuid::parse_str(text).map_err(|_| self.refuse(field)))
            .transpose()
    }

    fn optional_number<Number: std::str::FromStr>(
        &self,
        field: &'static str,
        value: &Option<String>,
    ) -> Result<Option<Number>, SnapshotRefusal> {
        value
            .as_deref()
            .map(|text| text.parse::<Number>().map_err(|_| self.refuse(field)))
            .transpose()
    }

    fn number<Number: std::str::FromStr>(
        &self,
        field: &'static str,
        value: &Option<String>,
    ) -> Result<Number, SnapshotRefusal> {
        self.optional_number(field, value)?
            .ok_or_else(|| self.refuse(field))
    }
}

fn storage(error: impl std::fmt::Display) -> SnapshotRefusal {
    SnapshotRefusal::Storage(error.to_string())
}

pub(crate) async fn read_durable<S>(
    scope: &mut S,
    fleet: bool,
    now: SyncTime,
) -> Result<DurableSnapshot, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let attempts = attempts(scope).await?;
    let in_flight = in_flight(scope).await?;
    if !fleet {
        // A Copy holds attempts only (seam contract v3.3).
        return Ok(DurableSnapshot {
            attempts,
            in_flight,
            ..DurableSnapshot::default()
        });
    }
    Ok(DurableSnapshot {
        attempts,
        in_flight,
        notice_progress: notice_progress(scope).await?,
        feed: Some(feed(scope, now).await?),
        owners: owners(scope).await?,
    })
}

async fn attempts<S>(scope: &mut S) -> Result<Vec<AttemptRecord>, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::read_snapshot_attempts(scope)
        .await
        .map_err(storage)?
        .into_iter()
        .map(|row| {
            let columns = Columns {
                table: ATTEMPTS,
                key: &row.id,
            };
            Ok(AttemptRecord {
                id: columns.uuid("id", &row.id)?,
                subject: columns.uuid("subject", &row.subject)?,
                send_id: columns.optional_uuid("send_id", &row.send_id)?,
                connection_id: columns.optional_uuid("connection_id", &row.connection_id)?,
                session_id: columns.optional_uuid("session_id", &row.session_id)?,
                sent_version: columns.optional_number("sent_version", &row.sent_version)?,
                next_attempt_at: columns
                    .optional_number("next_attempt_at", &row.next_attempt_at)?,
                attempts: columns.number("attempts", &row.attempts)?,
                first_failed_at: columns
                    .optional_number("first_failed_at", &row.first_failed_at)?,
                created_at: columns.number("created_at", &row.created_at)?,
                updated_at: columns.number("updated_at", &row.updated_at)?,
                kind: columns.text("kind", row.kind.clone())?,
                state: columns.text("state", row.state.clone())?,
                last_error_class: row.last_error_class,
                last_error: row.last_error,
            })
        })
        .collect()
}

async fn in_flight<S>(scope: &mut S) -> Result<Vec<(ConnectionId, Uuid, u64)>, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::read_snapshot_in_flight(scope)
        .await
        .map_err(storage)?
        .into_iter()
        .map(|row| {
            let columns = Columns {
                table: ATTEMPTS,
                key: &row.connection_id,
            };
            let connection = ConnectionId::new(columns.uuid("connection_id", &row.connection_id)?)
                .map_err(|_| columns.refuse("connection_id"))?;
            Ok((
                connection,
                columns.uuid("session_id", &row.session_id)?,
                columns.number("in_flight", &row.in_flight)?,
            ))
        })
        .collect()
}

async fn notice_progress<S>(scope: &mut S) -> Result<Vec<NoticeProgressRecord>, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::read_snapshot_notice_progress(scope)
        .await
        .map_err(storage)?
        .into_iter()
        .map(|row| {
            let columns = Columns {
                table: PROGRESS,
                key: &row.machine_id,
            };
            let machine = PeerId::new(columns.uuid("machine_id", &row.machine_id)?)
                .map_err(|_| columns.refuse("machine_id"))?;
            Ok(NoticeProgressRecord {
                machine,
                last_issued: columns.number("last_issued_position", &row.last_issued)?,
                expired_through: columns.number("expired_through", &row.expired_through)?,
                applied_through: columns
                    .optional_number("applied_through", &row.applied_through)?,
            })
        })
        .collect()
}

async fn feed<S>(scope: &mut S, now: SyncTime) -> Result<FeedRecord, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let state = crate::fleet_feed_store::state(scope)
        .await
        .map_err(storage)?;
    let now = chrono::DateTime::from_timestamp_millis(now.unix_millis())
        .ok_or_else(|| storage("the object's clock is out of range"))?;
    let effective = crate::fleet_feed_store::effective_floor(scope, &state, now)
        .await
        .map_err(storage)?;
    Ok(FeedRecord {
        head: state.head.get(),
        retained_through: state.floor.get(),
        effective_floor: effective.get(),
    })
}

async fn owners<S>(scope: &mut S) -> Result<OwnersRecord, SnapshotRefusal>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let owner_refusal = |error: fleet_mail_delivery::DeliveryError| match error {
        fleet_mail_delivery::DeliveryError::CorruptRow { table, key, field } => {
            SnapshotRefusal::CorruptRow { table, key, field }
        }
        other => storage(other),
    };
    let outbox = fleet_mail_delivery::outbox_owner_rows(scope)
        .await
        .map_err(owner_refusal)?
        .into_iter()
        .map(|row| OutboxOwnerRecord {
            item: row.item.as_uuid(),
            record_kind: row.record_kind,
            custody: row.custody,
        })
        .collect();
    let status_owed = fleet_mail_delivery::status_owed_rows(scope)
        .await
        .map_err(owner_refusal)?
        .into_iter()
        .map(|row| StatusOwedRecord {
            id: row.id,
            item: row.item.as_uuid(),
            version: row.version,
        })
        .collect();
    Ok(OwnersRecord {
        outbox,
        status_owed,
    })
}
