//! Ownership-run metadata bounds delivery's local intent scan.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{EpochEnd, EpochNumber, FeedCursor, FeedPosition, ServiceId, WriterEpoch};

struct EpochMetadata {
    epoch: i64,
    holder: String,
    start_position: i64,
    ended: Option<String>,
}
impl EpochMetadata {
    fn decode(
        self,
        project: &message_board::ProjectId,
    ) -> Result<WriterEpoch, ProjectStorageError> {
        let invalid = || ProjectStorageError::InvalidFeed;
        Ok(WriterEpoch {
            project: project.clone(),
            number: EpochNumber::try_from(u64::try_from(self.epoch).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
            holder: ServiceId::try_from(self.holder).map_err(|_| invalid())?,
            start_position: FeedPosition::try_from(
                u64::try_from(self.start_position).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?,
            end: self
                .ended
                .map(|ended| serde_json::from_str::<EpochEnd>(&ended).map_err(|_| invalid()))
                .transpose()?,
        })
    }
}
impl ProjectSnapshot {
    pub async fn intent_scan_floor(
        &mut self,
        installation: &ServiceId,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        let epoch = i64::try_from(self.high_water.epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let current = sqlx_turso::query_as!(EpochMetadata, r#"SELECT epoch AS "epoch!: i64", holder AS "holder!: String", start_position AS "start_position!: i64", ended AS "ended: String" FROM writer_epochs WHERE epoch = ?"#, epoch).fetch_optional(&mut *self.transaction).await?.ok_or(ProjectStorageError::InvalidFeed)?.decode(&self.project_id)?;
        if current.start_position > self.high_water.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if &current.holder != installation || current.end.is_some() {
            return Ok(None);
        }
        let installation = installation.as_str();
        let previous = sqlx_turso::query_as!(EpochMetadata, r#"SELECT epoch AS "epoch!: i64", holder AS "holder!: String", start_position AS "start_position!: i64", ended AS "ended: String" FROM writer_epochs WHERE epoch < ? AND holder != ? ORDER BY epoch DESC LIMIT 1"#, epoch, installation).fetch_optional(&mut *self.transaction).await?;
        let previous = previous
            .map(|row| row.decode(&self.project_id))
            .transpose()?;
        let boundary = previous.as_ref().map_or(0, |row| row.number.get());
        let boundary = i64::try_from(boundary).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let floor = sqlx_turso::query_as!(EpochMetadata, r#"SELECT epoch AS "epoch!: i64", holder AS "holder!: String", start_position AS "start_position!: i64", ended AS "ended: String" FROM writer_epochs WHERE epoch > ? AND epoch <= ? ORDER BY epoch LIMIT 1"#, boundary, epoch).fetch_one(&mut *self.transaction).await?.decode(&self.project_id)?;
        if floor.holder.as_str() != installation
            || floor.start_position > current.start_position
            || previous.is_some_and(|row| row.start_position > floor.start_position)
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let floor_number =
            i64::try_from(floor.number.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM writer_epochs WHERE epoch >= ? AND epoch <= ?"#, floor_number, epoch).fetch_one(&mut *self.transaction).await?;
        if count != epoch - floor_number + 1 {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(Some(FeedCursor {
            epoch: floor.number,
            position: floor.start_position,
        }))
    }
}
impl ProjectWriteTransaction {
    pub async fn record_writer_epoch(
        &mut self,
        epoch: &WriterEpoch,
    ) -> Result<(), ProjectStorageError> {
        if epoch.project != self.project_id {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let number =
            i64::try_from(epoch.number.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let holder = epoch.holder.as_str();
        let start = i64::try_from(epoch.start_position.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let ended = epoch.end.as_ref().map(serde_json::to_string).transpose()?;
        sqlx_turso::query!(
            "INSERT INTO writer_epochs(epoch, holder, start_position, ended) VALUES (?, ?, ?, ?)",
            number,
            holder,
            start,
            ended
        )
        .execute(&mut *self.transaction)
        .await?;
        Ok(())
    }
}
