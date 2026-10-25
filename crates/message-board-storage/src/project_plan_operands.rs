//! Current plan selection uses metadata without decoding unrelated revisions.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

fn reference(kind: RecordKind, id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
async fn current_plan_key<TScope>(
    connection: &mut TScope,
) -> Result<Option<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    sqlx_turso::query_scalar!(r#"SELECT revision_id AS "id!: String" FROM plan_revisions WHERE superseded_by IS NULL ORDER BY revision DESC, revision_id DESC LIMIT 1"#).fetch_optional(connection).await?.map(|id| reference(RecordKind::PlanRevision, id)).transpose()
}
impl ProjectWriteTransaction {
    pub async fn current_plan_key(&mut self) -> Result<Option<RecordRef>, ProjectStorageError> {
        current_plan_key(&mut self.transaction).await
    }
}
impl ProjectSnapshot {
    pub async fn current_plan_key(&mut self) -> Result<Option<RecordRef>, ProjectStorageError> {
        current_plan_key(&mut self.transaction).await
    }
}
