//! A replacement projection stays private until snapshot and feed catch-up finish.
use super::{FleetCache, cache_checkpoint, record_decode_error};
use crate::{
    RouterLocalStorageError,
    fleet_record_codec::{decode_fleet_record, decode_fleet_reference, validate_fleet_record},
    project_record_io::record_kind_tag,
};
use fleet_admission::FleetFeedEntry;
use message_board::{FleetPosition, Record, RecordRef};
use tokio::sync::MutexGuard;

enum RebuildStage {
    Snapshot {
        after: Option<RecordRef>,
    },
    CatchUp {
        through: FleetPosition,
        head: FleetPosition,
    },
    Ready {
        through: FleetPosition,
    },
    Aborted,
}

pub struct FleetCacheRebuild<'cache> {
    cache: &'cache FleetCache,
    _writer: MutexGuard<'cache, ()>,
    snapshot_at: FleetPosition,
    stage: RebuildStage,
}
impl FleetCache {
    pub async fn begin_snapshot_rebuild(
        &self,
        snapshot_at: FleetPosition,
    ) -> Result<FleetCacheRebuild<'_>, RouterLocalStorageError> {
        let writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let _ = cache_checkpoint(&mut transaction).await?;
            // There is one rebuild writer. Rows left by a dropped or crashed
            // build are disposable; the active projection is untouched.
            sqlx_turso::query!("DELETE FROM fleet_cache_rebuild_records")
                .execute(&mut *transaction)
                .await?;
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await?,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        }
        Ok(FleetCacheRebuild {
            cache: self,
            _writer: writer,
            snapshot_at,
            stage: RebuildStage::Snapshot { after: None },
        })
    }
}
impl FleetCacheRebuild<'_> {
    pub async fn append_snapshot_page(
        &mut self,
        snapshot_at: FleetPosition,
        records: &[Record],
        next: Option<RecordRef>,
    ) -> Result<(), RouterLocalStorageError> {
        let RebuildStage::Snapshot { mut after } =
            std::mem::replace(&mut self.stage, RebuildStage::Aborted)
        else {
            return Err(RouterLocalStorageError::InvalidCursor);
        };
        if snapshot_at != self.snapshot_at {
            return Err(RouterLocalStorageError::InvalidCursor);
        }
        for record in records {
            validate_fleet_record(record).map_err(record_decode_error)?;
            let reference = record.reference();
            if after
                .as_ref()
                .is_some_and(|previous| key(previous) >= key(&reference))
            {
                return Err(RouterLocalStorageError::InvalidCursor);
            }
            after = Some(reference);
        }
        if next
            .as_ref()
            .is_some_and(|next| records.is_empty() || after.as_ref() != Some(next))
        {
            return Err(RouterLocalStorageError::InvalidCursor);
        }
        let mut transaction = self
            .cache
            .database
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await?;
        let result = async {
            for record in records {
                put_staged_record(&mut transaction, record).await?;
            }
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await?,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        }
        self.stage = match next {
            Some(_) => RebuildStage::Snapshot { after },
            None => RebuildStage::CatchUp {
                through: self.snapshot_at,
                head: self.snapshot_at,
            },
        };
        Ok(())
    }
    pub async fn apply_catchup_page(
        &mut self,
        entries: &[FleetFeedEntry],
        through: FleetPosition,
        head: FleetPosition,
    ) -> Result<(), RouterLocalStorageError> {
        let RebuildStage::CatchUp {
            through: after,
            head: previous_head,
        } = std::mem::replace(&mut self.stage, RebuildStage::Aborted)
        else {
            return Err(RouterLocalStorageError::InvalidCursor);
        };
        if through < after
            || through > head
            || head < previous_head
            || (through == after && through != head)
        {
            return Err(RouterLocalStorageError::PositionGap);
        }
        let mut previous = after;
        for entry in entries {
            if previous
                .successor()
                .map_err(|_| RouterLocalStorageError::InvalidRecord)?
                != entry.position
                || entry.record.envelope().position.fleet_position() != Some(entry.position)
            {
                return Err(RouterLocalStorageError::PositionGap);
            }
            validate_fleet_record(&entry.record).map_err(record_decode_error)?;
            previous = entry.position;
        }
        if previous != through {
            return Err(RouterLocalStorageError::PositionGap);
        }
        let mut transaction = self
            .cache
            .database
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await?;
        let result = async {
            // A snapshot page may contain a newer current row than its cut.
            // Replay is deliberately last-write-wins inside private staging.
            for entry in entries {
                put_staged_record(&mut transaction, &entry.record).await?;
            }
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await?,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        }
        self.stage = if through == head {
            RebuildStage::Ready { through }
        } else {
            RebuildStage::CatchUp { through, head }
        };
        Ok(())
    }
    pub async fn publish(self) -> Result<(), RouterLocalStorageError> {
        let RebuildStage::Ready { through } = self.stage else {
            return Err(RouterLocalStorageError::InvalidCursor);
        };
        let mut transaction = self
            .cache
            .database
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await?;
        let result = async {
            validate_staged_records(&mut transaction, through).await?;
            let _ = cache_checkpoint(&mut transaction).await?;
            sqlx_turso::query!("DELETE FROM cached_fleet_records").execute(&mut *transaction).await?;
            sqlx_turso::query!("INSERT INTO cached_fleet_records(kind,id,payload) SELECT kind,id,payload FROM fleet_cache_rebuild_records")
                .execute(&mut *transaction).await?;
            let position=i64::try_from(through.get()).map_err(|_|RouterLocalStorageError::InvalidRecord)?;
            sqlx_turso::query!("UPDATE fleet_cache_checkpoint SET position=? WHERE singleton=1",position)
                .execute(&mut *transaction).await?;
            sqlx_turso::query!("DELETE FROM fleet_cache_rebuild_records").execute(&mut *transaction).await?;
            Ok::<(),RouterLocalStorageError>(())
        }.await;
        match result {
            Ok(()) => Ok(transaction.commit().await?),
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

fn key(reference: &RecordRef) -> (&str, &'static str) {
    (reference.id.as_str(), record_kind_tag(reference.kind))
}
#[cfg(test)]
#[path = "fleet_cache_rebuild/identity_tests.rs"]
mod identity_tests;
async fn put_staged_record(
    connection: &mut sqlx_turso::TursoConnection,
    record: &Record,
) -> Result<(), RouterLocalStorageError> {
    let reference = record.reference();
    let id = reference.id.as_str();
    let kind = record_kind_tag(reference.kind);
    let payload = serde_json::to_string(record)?;
    let row_id = uuid::Uuid::now_v7().to_string();
    sqlx_turso::query!("INSERT INTO fleet_cache_rebuild_records(row_id,id,kind,payload) VALUES(?,?,?,?) ON CONFLICT(id,kind) DO UPDATE SET payload=excluded.payload",row_id,id,kind,payload)
        .execute(connection).await?;
    Ok(())
}
async fn validate_staged_records(
    connection: &mut sqlx_turso::TursoConnection,
    through: FleetPosition,
) -> Result<(), RouterLocalStorageError> {
    // Sixteen rows keeps validation bounded even for records near the 1 MiB
    // record budget. There is no network await in this publication transaction.
    let mut after_id = String::new();
    let mut after_kind = String::new();
    loop {
        let rows=sqlx_turso::query!(r#"SELECT row_id AS "row_id!: String",id AS "id!: String",kind AS "kind!: String",payload AS "payload!: String" FROM fleet_cache_rebuild_records WHERE id>?1 OR (id=?1 AND kind>?2) ORDER BY id,kind LIMIT 16"#,after_id,after_kind)
            .fetch_all(&mut *connection).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let physical_id = uuid::Uuid::parse_str(&row.row_id)
                .map_err(|_| RouterLocalStorageError::InvalidRecord)?;
            if physical_id.get_version_num() != 7 || physical_id.to_string() != row.row_id {
                return Err(RouterLocalStorageError::InvalidRecord);
            }
            let reference =
                decode_fleet_reference(&row.kind, row.id.clone()).map_err(record_decode_error)?;
            if reference.id.as_str() != row.id || record_kind_tag(reference.kind) != row.kind {
                return Err(RouterLocalStorageError::InvalidRecord);
            }
            let record =
                decode_fleet_record(&reference, &row.payload).map_err(record_decode_error)?;
            if record
                .envelope()
                .position
                .fleet_position()
                .is_none_or(|position| position > through)
            {
                return Err(RouterLocalStorageError::PositionGap);
            }
            after_id = row.id;
            after_kind = row.kind;
        }
    }
    Ok(())
}
