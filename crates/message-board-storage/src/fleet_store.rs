//! Native Router stand-in adapter; the pure decision crate owns the rules.
//!
//! **Superseded (2026-10-16).** This store admits fleet requests inside the Router: it reads a
//! snapshot, decides, applies and replies. That makes it a third realization of central's fleet
//! admission, alongside central's two runtimes. Under the one-core design, the Router reaches
//! fleet admission only over its machine session to central (requirements U69, U20). The
//! Router's outbox, cache and replay stay. See `docs/specs/2026-10-16-central-one-core/program-design.md`.
use crate::{
    FleetStorageError,
    fleet_record_codec::{decode_fleet_record, decode_fleet_reference, read_fleet_record},
};
use chrono::{DateTime, Utc};
use fleet_admission::{FleetAdmissionError, FleetDecision, FleetIds, FleetRequest, FleetSnapshot};
use message_board::{FleetPosition, Record, RecordRef, RequestResult};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Pool, pool::PoolOptions};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;
use tokio::sync::Mutex;

pub static FLEET_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./fleet_migrations");

pub struct FleetStore {
    authoritative: Pool<Turso>,
    reader: Pool<Turso>,
    writer_mutex: Mutex<()>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetReply {
    pub result: RequestResult,
    pub records: Vec<Record>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetRecordSnapshot {
    pub through: FleetPosition,
    pub records: Vec<Record>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetFeedPage {
    pub entries: Vec<fleet_admission::FleetFeedEntry>,
    pub covered_through: FleetPosition,
    pub high_water: FleetPosition,
}

impl FleetStore {
    pub async fn report_archive_seen(
        &self,
        report: &fleet_admission::ArchiveReport,
    ) -> Result<fleet_admission::ArchiveReportOutcome, FleetStorageError> {
        let _guard = self.writer_mutex.lock().await;
        let mut transaction = self.authoritative.begin_with("BEGIN IMMEDIATE").await?;
        match crate::fleet_archive_reports::receive(&mut transaction, report).await {
            Ok(outcome) => {
                transaction.commit().await?;
                Ok(outcome)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    pub async fn open(path: &Path) -> Result<Self, FleetStorageError> {
        let options = TursoConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let authoritative = PoolOptions::<Turso>::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(options.clone())
            .await?;
        let mut connection = authoritative.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *connection)
            .await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let migration = async {
            FLEET_MIGRATOR
                .run_direct(None, &mut *transaction, false)
                .await
                .map_err(|_| FleetStorageError::InvalidRecord)?;
            if !sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut *transaction)
                .await?
                .is_empty()
            {
                return Err(FleetStorageError::InvalidRecord);
            }
            Ok::<(), FleetStorageError>(())
        }
        .await;
        let migration_result = match migration {
            Ok(()) => transaction
                .commit()
                .await
                .map_err(FleetStorageError::Database),
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        };
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *connection)
            .await?;
        migration_result?;
        drop(connection);
        let reader = PoolOptions::<Turso>::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(options)
            .await?;
        Ok(Self {
            authoritative,
            reader,
            writer_mutex: Mutex::new(()),
        })
    }

    pub async fn admit(
        &self,
        request: &FleetRequest,
        accepted_at: DateTime<Utc>,
        ids: &FleetIds,
    ) -> Result<FleetReply, FleetStorageError> {
        let _guard = self.writer_mutex.lock().await;
        let mut transaction = self.authoritative.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let snapshot =
                crate::fleet_snapshot_reads::read_snapshot(&mut transaction, request).await?;
            let decision = fleet_admission::decide(request, &snapshot, accepted_at, ids)?;
            crate::fleet_decision_writes::apply_decision(&mut transaction, &decision).await?;
            reply_for(&mut transaction, &decision, &snapshot).await
        }
        .await;
        match result {
            Ok(reply) => {
                transaction.commit().await?;
                Ok(reply)
            }
            Err(FleetStorageError::Admission(FleetAdmissionError::ConflictingRequest)) => {
                transaction.rollback().await?;
                Ok(FleetReply {
                    result: RequestResult::ConflictingRequest,
                    records: Vec::new(),
                })
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    pub async fn record(&self, reference: &RecordRef) -> Result<Option<Record>, FleetStorageError> {
        let mut connection = self.reader.acquire().await?;
        read_fleet_record(&mut connection, reference).await
    }

    pub async fn snapshot_records(&self) -> Result<FleetRecordSnapshot, FleetStorageError> {
        let mut transaction = self.reader.begin().await?;
        let through = checkpoint(&mut transaction).await?;
        let rows = sqlx_turso::query!(r#"SELECT kind AS "kind!: String", id AS "id!: String", payload AS "payload!: String" FROM fleet_records ORDER BY kind, id"#).fetch_all(&mut *transaction).await?;
        let records = rows
            .into_iter()
            .map(|row| {
                let reference = decode_fleet_reference(&row.kind, row.id)?;
                decode_fleet_record(&reference, &row.payload)
            })
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await?;
        Ok(FleetRecordSnapshot { through, records })
    }
    pub async fn feed_after(
        &self,
        after: FleetPosition,
        limit: u64,
    ) -> Result<FleetFeedPage, FleetStorageError> {
        let mut transaction = self.reader.begin().await?;
        let high_water = checkpoint(&mut transaction).await?;
        if limit == 0 || after > high_water {
            return Err(FleetStorageError::InvalidRecord);
        }
        let top = after.get().saturating_add(limit).min(high_water.get());
        let start = i64::try_from(after.get()).map_err(|_| FleetStorageError::InvalidRecord)?;
        let end = i64::try_from(top).map_err(|_| FleetStorageError::InvalidRecord)?;
        let rows = sqlx_turso::query!(r#"SELECT position AS "position!: i64", entry AS "entry!: String" FROM fleet_feed WHERE position > ? AND position <= ? ORDER BY position"#, start, end).fetch_all(&mut *transaction).await?;
        let mut entries = Vec::new();
        let mut previous = after;
        for row in rows {
            tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
            let entry: fleet_admission::FleetFeedEntry =
                serde_json::from_str(&row.entry).map_err(|_| FleetStorageError::InvalidRecord)?;
            crate::project_record_io::validate_canonical_project_text(
                &row.entry,
                &entry,
                &entry.record.reference(),
            )
            .map_err(|_| FleetStorageError::InvalidRecordAt {
                record: entry.record.reference(),
            })?;
            let expected = previous
                .successor()
                .map_err(|_| FleetStorageError::InvalidRecord)?;
            if entry.position != expected
                || row.position
                    != i64::try_from(expected.get())
                        .map_err(|_| FleetStorageError::InvalidRecord)?
                || entry.record.envelope().position
                    != message_board::RecordPosition::Fleet(expected)
            {
                return Err(FleetStorageError::InvalidRecord);
            }
            crate::fleet_record_codec::validate_fleet_record(&entry.record)?;
            previous = entry.position;
            entries.push(entry);
        }
        if previous.get() != top {
            return Err(FleetStorageError::InvalidRecord);
        }
        transaction.commit().await?;
        Ok(FleetFeedPage {
            entries,
            covered_through: previous,
            high_water,
        })
    }
    pub async fn close(self) {
        self.authoritative.close().await;
        self.reader.close().await;
    }
}

pub(crate) async fn checkpoint(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<FleetPosition, FleetStorageError> {
    let row = sqlx_turso::query!(r#"SELECT last_position AS "last_position!: i64", (SELECT COUNT(*) FROM fleet_checkpoint) AS "count!: i64" FROM fleet_checkpoint WHERE singleton = 1"#).fetch_one(connection).await?;
    if row.count != 1 {
        return Err(FleetStorageError::InvalidRecord);
    }
    let value = u64::try_from(row.last_position).map_err(|_| FleetStorageError::InvalidRecord)?;
    FleetPosition::try_from(value).map_err(|_| FleetStorageError::InvalidRecord)
}

async fn reply_for(
    connection: &mut sqlx_turso::TursoConnection,
    decision: &FleetDecision,
    snapshot: &FleetSnapshot,
) -> Result<FleetReply, FleetStorageError> {
    let mut returned = Vec::new();
    if let message_board::RequestDecision::Accepted {
        records, positions, ..
    } = &decision.decision
    {
        if records.len() != positions.len() {
            return Err(FleetStorageError::InvalidRecord);
        }
        for (reference, position) in records.iter().zip(positions) {
            let message_board::RecordPosition::Fleet(cursor) = position else {
                return Err(FleetStorageError::InvalidRecord);
            };
            if let Some(record) = decision
                .feed
                .iter()
                .find(|entry| {
                    entry.record.reference() == *reference
                        && entry.record.envelope().position == *position
                })
                .map(|entry| &entry.record)
                .or_else(|| {
                    snapshot
                        .record(reference)
                        .filter(|record| record.envelope().position == *position)
                })
            {
                returned.push(record.clone());
            } else {
                let stored_position =
                    i64::try_from(cursor.get()).map_err(|_| FleetStorageError::InvalidRecord)?;
                let encoded = sqlx_turso::query_scalar!(
                    r#"SELECT entry AS "entry!: String" FROM fleet_feed WHERE position = ?"#,
                    stored_position
                )
                .fetch_one(&mut *connection)
                .await?;
                let entry: fleet_admission::FleetFeedEntry =
                    serde_json::from_str(&encoded).map_err(|_| FleetStorageError::InvalidRecord)?;
                crate::project_record_io::validate_canonical_project_text(
                    &encoded,
                    &entry,
                    &entry.record.reference(),
                )
                .map_err(|_| FleetStorageError::InvalidRecordAt {
                    record: entry.record.reference(),
                })?;
                crate::fleet_record_codec::validate_fleet_record(&entry.record)?;
                if entry.position != *cursor
                    || entry.record.reference() != *reference
                    || entry.record.envelope().position != *position
                {
                    return Err(FleetStorageError::InvalidRecord);
                }
                returned.push(entry.record);
            }
        }
    }
    Ok(FleetReply {
        result: RequestResult::Decided(decision.decision.clone()),
        records: returned,
    })
}
