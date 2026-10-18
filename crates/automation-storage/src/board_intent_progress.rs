//! C5 consumes versions of intent state; NI-2 can reinstate the same identity.
use crate::{AutomationStore, StorageError};
use message_board::{EpochNumber, FeedCursor, FeedPosition, NotifyIntentId, ProjectId};

impl AutomationStore {
    pub async fn intents_scanned_through(
        &mut self,
        project: &ProjectId,
    ) -> Result<Option<FeedCursor>, StorageError> {
        let project = project.as_str();
        let row = sqlx::query!(
            "SELECT epoch, position FROM board_intent_scan_cursors WHERE project_id = ?",
            project
        )
        .fetch_optional(&mut self.connection)
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
        &mut self,
        project: &ProjectId,
        through: FeedCursor,
    ) -> Result<(), StorageError> {
        let project = project.as_str();
        let epoch = i64::try_from(through.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(through.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        let result = sqlx::query!("INSERT INTO board_intent_scan_cursors(project_id, epoch, position) VALUES (?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET epoch = excluded.epoch, position = CASE WHEN excluded.epoch = board_intent_scan_cursors.epoch THEN max(board_intent_scan_cursors.position, excluded.position) ELSE excluded.position END WHERE excluded.epoch >= board_intent_scan_cursors.epoch", project, epoch, position)
            .execute(&mut self.connection).await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InvalidRecord);
        }
        Ok(())
    }

    pub async fn consumed_intents(
        &mut self,
        project: &ProjectId,
        intents: &[(NotifyIntentId, FeedCursor)],
    ) -> Result<std::collections::HashSet<(NotifyIntentId, FeedCursor)>, StorageError> {
        let project = project.as_str();
        let keys = serde_json::to_string(intents).map_err(|_| StorageError::InvalidRecord)?;
        let rows = sqlx::query!(r#"SELECT p.intent_id AS "intent_id!: String", p.state_epoch AS "state_epoch!: i64", p.state_position AS "state_position!: i64" FROM json_each(?) k CROSS JOIN board_intent_progress p ON p.project_id = ? AND p.intent_id = CAST(json_extract(k.value, '$[0]') AS TEXT) AND p.state_epoch = CAST(json_extract(k.value, '$[1].epoch') AS INTEGER) AND p.state_position = CAST(json_extract(k.value, '$[1].position') AS INTEGER) ORDER BY p.intent_id, p.state_epoch, p.state_position"#, keys, project)
            .fetch_all(&mut self.connection).await?;
        rows.into_iter()
            .map(|row| {
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
        &mut self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<bool, StorageError> {
        let project_key = project.as_str();
        let intent_key = intent.as_str();
        let epoch =
            i64::try_from(state_cursor.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(state_cursor.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        Ok(sqlx::query_scalar!("SELECT intent_id FROM board_intent_progress WHERE project_id = ? AND intent_id = ? AND state_epoch = ? AND state_position = ?", project_key, intent_key, epoch, position)
            .fetch_optional(&mut self.connection).await?.is_some())
    }

    pub async fn mark_intent_consumed(
        &mut self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        state_cursor: FeedCursor,
    ) -> Result<(), StorageError> {
        let project_key = project.as_str();
        let intent_key = intent.as_str();
        let epoch =
            i64::try_from(state_cursor.epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
        let position =
            i64::try_from(state_cursor.position.get()).map_err(|_| StorageError::InvalidRecord)?;
        sqlx::query!("INSERT INTO board_intent_progress(project_id, intent_id, state_epoch, state_position) VALUES (?, ?, ?, ?) ON CONFLICT(project_id, intent_id, state_epoch, state_position) DO NOTHING", project_key, intent_key, epoch, position)
            .execute(&mut self.connection).await?;
        Ok(())
    }
}
