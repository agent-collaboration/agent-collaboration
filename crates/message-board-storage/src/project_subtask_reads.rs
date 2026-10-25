//! Checklist membership is separate from the retained identities used by feed history.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::SubtaskId;

async fn live_subtask_ids<TScope>(
    connection: &mut TScope,
) -> Result<Vec<SubtaskId>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let rows = sqlx_turso::query_scalar!(
        r#"SELECT subtask_id AS "subtask_id!: String" FROM subtasks WHERE removed=0 ORDER BY subtask_id"#
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
            r#"SELECT subtask_id AS "subtask_id!: String" FROM subtasks WHERE removed=0 AND subtask_id > ? ORDER BY subtask_id LIMIT ?"#,
            after,
            limit
        )
        .fetch_all(&mut self.transaction)
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
}
