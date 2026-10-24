//! C5 consumes versions of intent state; NI-2 can reinstate the same identity.
use crate::{InteractionStore, StorageError};
use message_board::{EpochNumber, FeedCursor, FeedPosition, NotifyIntentId, ProjectId};

impl InteractionStore {
    pub async fn intents_scanned_through(
        &self,
        project: &ProjectId,
    ) -> Result<Option<FeedCursor>, StorageError> {
        let mut connection = self.writer.lock().await;
        let project = project.as_str();
        let row = sqlx_turso::query!(
            r#"SELECT epoch AS "epoch!: i64", position AS "position!: i64" FROM board_intent_scan_cursors WHERE project_id = ?"#,
            project
        )
        .fetch_optional(&mut *connection)
        .await?;
        row.map(|row| {
            Ok(FeedCursor {
                epoch: EpochNumber::try_from(
                    u64::try_from(row.epoch).map_err(|_| StorageError::InvalidRecord)?,
                )
                .map_err(|_| StorageError::InvalidRecord)?,
                position: FeedPosition::try_from(
                    u64::try_from(row.position).map_err(|_| StorageError::InvalidRecord)?,
                )
                .map_err(|_| StorageError::InvalidRecord)?,
            })
        })
        .transpose()
    }

    pub async fn advance_intents_scanned_through(
        &self,
        project: &ProjectId,
        through: FeedCursor,
    ) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        let project = project.as_str();
        let epoch = i64::try_from(through.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(through.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        let result = sqlx_turso::query!("INSERT INTO board_intent_scan_cursors(project_id, epoch, position) VALUES (?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET epoch = excluded.epoch, position = CASE WHEN excluded.epoch = board_intent_scan_cursors.epoch THEN max(board_intent_scan_cursors.position, excluded.position) ELSE excluded.position END WHERE excluded.epoch >= board_intent_scan_cursors.epoch", project, epoch, position)
            .execute(&mut *connection).await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InvalidRecord);
        }
        Ok(())
    }

    pub async fn consumed_intents(
        &self,
        project: &ProjectId,
        intents: &[(NotifyIntentId, FeedCursor)],
    ) -> Result<std::collections::HashSet<(NotifyIntentId, FeedCursor)>, StorageError> {
        let mut connection = self.writer.lock().await;
        let project = project.as_str();
        let keys = serde_json::to_string(intents).map_err(|_| StorageError::InvalidRecord)?;
        let rows = sqlx_turso::query!(r#"SELECT p.progress_id AS "progress_id!: String", p.intent_id AS "intent_id!: String", p.state_epoch AS "state_epoch!: i64", p.state_position AS "state_position!: i64" FROM json_each(?) k CROSS JOIN board_intent_progress p ON p.project_id = ? AND p.intent_id = CAST(json_extract(k.value, '$[0]') AS TEXT) AND p.state_epoch = CAST(json_extract(k.value, '$[1].epoch') AS INTEGER) AND p.state_position = CAST(json_extract(k.value, '$[1].position') AS INTEGER) ORDER BY p.intent_id, p.state_epoch, p.state_position"#, keys, project)
            .fetch_all(&mut *connection).await?;
        rows.into_iter()
            .map(|row| {
                crate::worker_progress_id::decode_worker_progress_id(row.progress_id)?;
                Ok((
                    NotifyIntentId::try_from(row.intent_id)
                        .map_err(|_| StorageError::InvalidRecord)?,
                    FeedCursor {
                        epoch: EpochNumber::try_from(
                            u64::try_from(row.state_epoch)
                                .map_err(|_| StorageError::InvalidRecord)?,
                        )
                        .map_err(|_| StorageError::InvalidRecord)?,
                        position: FeedPosition::try_from(
                            u64::try_from(row.state_position)
                                .map_err(|_| StorageError::InvalidRecord)?,
                        )
                        .map_err(|_| StorageError::InvalidRecord)?,
                    },
                ))
            })
            .collect()
    }

    pub async fn intent_consumed(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<bool, StorageError> {
        let mut connection = self.writer.lock().await;
        let project_key = project.as_str();
        let intent_key = intent.as_str();
        let epoch =
            i64::try_from(state_cursor.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(state_cursor.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        let progress_id = sqlx_turso::query_scalar!(r#"SELECT progress_id AS "progress_id!: String" FROM board_intent_progress WHERE project_id = ? AND intent_id = ? AND state_epoch = ? AND state_position = ?"#, project_key, intent_key, epoch, position)
            .fetch_optional(&mut *connection).await?;
        progress_id
            .map(crate::worker_progress_id::decode_worker_progress_id)
            .transpose()
            .map(|record| record.is_some())
    }

    pub async fn mark_intent_consumed(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        let project_key = project.as_str();
        let intent_key = intent.as_str();
        let epoch =
            i64::try_from(state_cursor.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(state_cursor.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        let proposed_id = uuid::Uuid::now_v7().to_string();
        let inserted = sqlx_turso::query!(r#"INSERT INTO board_intent_progress(progress_id, project_id, intent_id, state_epoch, state_position) VALUES (?, ?, ?, ?, ?) ON CONFLICT(project_id, intent_id, state_epoch, state_position) DO NOTHING RETURNING progress_id AS "progress_id!: String""#, proposed_id, project_key, intent_key, epoch, position)
            .fetch_optional(&mut *connection).await?;
        let progress_id = match inserted {
            Some(row) => row.progress_id,
            None => sqlx_turso::query_scalar!(r#"SELECT progress_id AS "progress_id!: String" FROM board_intent_progress WHERE project_id = ? AND intent_id = ? AND state_epoch = ? AND state_position = ?"#, project_key, intent_key, epoch, position)
                .fetch_one(&mut *connection).await?,
        };
        crate::worker_progress_id::decode_worker_progress_id(progress_id)?;
        Ok(())
    }
}
