//! Project watch cursors belong to the physical project file.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{EpochNumber, FeedCursor, FeedPosition, Identity, ProjectId};

async fn project_watch(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    reader: &Identity,
) -> Result<Option<FeedCursor>, ProjectStorageError> {
    let reader = serde_json::to_string(reader)?;
    let row = sqlx_turso::query!(r#"SELECT start_epoch AS "start_epoch!: i64", start_position AS "start_position!: i64", active AS "active!: i64" FROM project_watches WHERE reader = ?"#,reader).fetch_optional(&mut *connection).await?;
    let invalid = || ProjectStorageError::InvalidRecord {
        record: project.clone().into(),
        field: "projectWatch",
    };
    let Some(row) = row else { return Ok(None) };
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(u64::try_from(row.start_epoch).map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        position: FeedPosition::try_from(u64::try_from(row.start_position).map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
    };
    let high_water = crate::project_store::checkpoint(connection).await?;
    if cursor.epoch == high_water.epoch && cursor.position > high_water.position {
        return Err(invalid());
    }
    match row.active {
        0 => Ok(None),
        1 => Ok(Some(cursor)),
        _ => Err(invalid()),
    }
}
impl ProjectSnapshot {
    pub async fn has_reader_interest(
        &mut self,
        reader: &Identity,
    ) -> Result<bool, ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let present = sqlx_turso::query_scalar!(
            r#"SELECT (EXISTS(SELECT 1 FROM read_marks WHERE reader = ?) OR EXISTS(SELECT 1 FROM project_watches WHERE reader = ? AND active = 1) OR EXISTS(SELECT 1 FROM thread_watches WHERE reader = ? AND active = 1)) AS "present!: i64""#,
            reader,
            reader,
            reader
        ).fetch_one(&mut *self.transaction).await?;
        match present {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProjectStorageError::InvalidRecord {
                record: self.project_id.clone().into(),
                field: "readerInterest",
            }),
        }
    }

    pub async fn project_watch(
        &mut self,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        project_watch(&mut self.transaction, &self.project_id, reader).await
    }
}
impl ProjectWriteTransaction {
    pub async fn live_project_watchers(&mut self) -> Result<Vec<Identity>, ProjectStorageError> {
        let rows = sqlx_turso::query_scalar!(
            r#"SELECT reader AS "reader!: String" FROM project_watches WHERE active = 1 ORDER BY reader"#
        )
        .fetch_all(&mut *self.transaction)
        .await?;
        rows.into_iter()
            .map(|reader| {
                serde_json::from_str(&reader).map_err(|_| ProjectStorageError::InvalidRecord {
                    record: self.project_id.clone().into(),
                    field: "projectWatch.reader",
                })
            })
            .collect()
    }

    pub async fn project_watch(
        &mut self,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        project_watch(&mut self.transaction, &self.project_id, reader).await
    }
    pub async fn set_project_watch(
        &mut self,
        reader: &Identity,
        starts_after: FeedCursor,
        active: bool,
    ) -> Result<(), ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let epoch = i64::try_from(starts_after.epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let position = i64::try_from(starts_after.position.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row_count = sqlx_turso::query_scalar!(
            r#"SELECT COUNT(*) AS "row_count!: i64" FROM project_watches WHERE reader = ?"#,
            reader
        )
        .fetch_one(&mut *self.transaction)
        .await?;
        match row_count {
            0 => {
                sqlx_turso::query!("INSERT INTO project_watches(reader, start_epoch, start_position, active) VALUES (?, ?, ?, ?)",reader,epoch,position,active).execute(&mut *self.transaction).await?;
            }
            1 => {
                sqlx_turso::query!("UPDATE project_watches SET start_epoch = CASE WHEN active = 1 AND ? = 1 THEN start_epoch ELSE ? END, start_position = CASE WHEN active = 1 AND ? = 1 THEN start_position ELSE ? END, active = ? WHERE reader = ?",active,epoch,active,position,active,reader).execute(&mut *self.transaction).await?;
            }
            _ => return Err(ProjectStorageError::InvalidFeed),
        }
        Ok(())
    }
}
