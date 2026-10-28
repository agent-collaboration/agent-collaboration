//! Assignment revisions count changes, not the number of holders retained in history.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{AssignmentRevision, RoleSubject};

async fn assignment_revision<TScope>(
    connection: &mut TScope,
    subject: &RoleSubject,
) -> Result<AssignmentRevision, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let count = match subject {
        RoleSubject::Project(project) => {
            let id = project.as_str();
            sqlx_turso::query_scalar!(r#"SELECT COUNT(DISTINCT CASE WHEN f.request_id IS NULL THEN '=' || f.record_id ELSE '?' || f.request_id END) AS "changes!: i64" FROM role_assignments a JOIN project_feed f ON f.record_kind = 'roleAssignment' AND f.record_id = a.assignment_id WHERE a.project_subject = ?"#, id).fetch_one(connection).await?
        }
        RoleSubject::Workstream(workstream) => {
            let id = workstream.as_str();
            sqlx_turso::query_scalar!(r#"SELECT COUNT(DISTINCT CASE WHEN f.request_id IS NULL THEN '=' || f.record_id ELSE '?' || f.request_id END) AS "changes!: i64" FROM role_assignments a JOIN project_feed f ON f.record_kind = 'roleAssignment' AND f.record_id = a.assignment_id WHERE a.workstream_subject = ?"#, id).fetch_one(connection).await?
        }
    };
    AssignmentRevision::try_from(
        u64::try_from(count).map_err(|_| ProjectStorageError::InvalidFeed)?,
    )
    .map_err(|_| ProjectStorageError::InvalidFeed)
}

impl ProjectWriteTransaction {
    pub async fn assignment_revision(
        &mut self,
        subject: &RoleSubject,
    ) -> Result<AssignmentRevision, ProjectStorageError> {
        assignment_revision(&mut self.transaction, subject).await
    }
}
impl ProjectSnapshot {
    pub async fn assignment_revision(
        &mut self,
        subject: &RoleSubject,
    ) -> Result<AssignmentRevision, ProjectStorageError> {
        assignment_revision(&mut self.transaction, subject).await
    }
}
