//! Checklist membership is separate from the retained identities used by feed history.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::SubtaskId;

async fn live_subtask_ids(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Vec<SubtaskId>, ProjectStorageError> {
    let rows = sqlx_turso::query_scalar!(
        r#"SELECT subtask_id AS "subtask_id!: String" FROM subtasks ORDER BY subtask_id"#
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|id| SubtaskId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed))
        .collect()
}
impl ProjectSnapshot {
    pub async fn live_subtask_page_keys(
        &mut self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<message_board::RecordRef>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query_scalar!(
            r#"SELECT subtask_id AS "subtask_id!: String" FROM subtasks WHERE subtask_id > ? ORDER BY subtask_id LIMIT ?"#,
            after,
            limit
        )
        .fetch_all(&mut *self.transaction)
        .await?;
        rows.into_iter()
            .map(|id| {
                SubtaskId::try_from(id)
                    .map(Into::into)
                    .map_err(|_| ProjectStorageError::InvalidFeed)
            })
            .collect()
    }
    pub async fn live_subtask_ids(&mut self) -> Result<Vec<SubtaskId>, ProjectStorageError> {
        live_subtask_ids(&mut self.transaction).await
    }
}
impl ProjectWriteTransaction {
    pub async fn live_subtask_ids(&mut self) -> Result<Vec<SubtaskId>, ProjectStorageError> {
        live_subtask_ids(&mut self.transaction).await
    }
    pub async fn remove_subtask(
        &mut self,
        subtask: &SubtaskId,
    ) -> Result<bool, ProjectStorageError> {
        let id = subtask.as_str();
        let result = sqlx_turso::query!("DELETE FROM subtasks WHERE subtask_id = ?", id)
            .execute(&mut *self.transaction)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}
