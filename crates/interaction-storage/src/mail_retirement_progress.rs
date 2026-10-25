//! Installation-local progress retries retirement below the acknowledged project frontier.
use crate::{InteractionStore, StorageError};
use message_board::{HandoffSeq, ProjectId};

async fn read_retirement_progress(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
) -> Result<HandoffSeq, StorageError> {
    let position = sqlx_turso::query_scalar!(
            r#"SELECT retired_through AS "retired_through!: i64" FROM mail_retirement_progress WHERE project_id=?"#,
            project.as_str()
        ).fetch_optional(connection).await?.unwrap_or(0);
    HandoffSeq::try_from(u64::try_from(position).map_err(|_| StorageError::InvalidStoredRecord)?)
        .map_err(|_| StorageError::InvalidStoredRecord)
}

impl InteractionStore {
    pub async fn mail_retired_through(
        &self,
        project: &ProjectId,
    ) -> Result<HandoffSeq, StorageError> {
        read_retirement_progress(&mut *self.writer.lock().await, project).await
    }

    pub async fn advance_mail_retired_through(
        &self,
        project: &ProjectId,
        through: HandoffSeq,
    ) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        if read_retirement_progress(&mut connection, project).await? > through {
            return Err(StorageError::InvalidStoredRecord);
        }
        let position = i64::try_from(through.get()).map_err(|_| StorageError::InvalidRecord)?;
        let changed = sqlx_turso::query!(
            "INSERT INTO mail_retirement_progress(project_id,retired_through) VALUES(?,?) ON CONFLICT(project_id) DO UPDATE SET retired_through=excluded.retired_through WHERE mail_retirement_progress.retired_through<=excluded.retired_through",
            project.as_str(), position
        ).execute(&mut *connection).await?;
        if changed.rows_affected() != 1 {
            return Err(StorageError::InvalidStoredRecord);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn retirement_progress_is_monotonic_idempotent_and_survives_physical_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        let project = ProjectId::generate();
        let owner = InteractionStore::open(&path).await.unwrap();
        assert_eq!(
            owner.mail_retired_through(&project).await.unwrap(),
            HandoffSeq::ZERO
        );
        for position in [2_u64, 2, 5] {
            owner
                .advance_mail_retired_through(&project, position.try_into().unwrap())
                .await
                .unwrap();
        }
        assert!(matches!(
            owner
                .advance_mail_retired_through(&project, 4.try_into().unwrap())
                .await,
            Err(StorageError::InvalidStoredRecord)
        ));
        owner.close().await.unwrap();
        let owner = InteractionStore::open(&path).await.unwrap();
        assert_eq!(owner.mail_retired_through(&project).await.unwrap().get(), 5);
        assert_eq!(
            owner
                .mail_retired_through(&ProjectId::generate())
                .await
                .unwrap(),
            HandoffSeq::ZERO
        );
        let mut writer = owner.writer.lock().await;
        sqlx::query("UPDATE mail_retirement_progress SET retired_through=-1")
            .execute(&mut *writer)
            .await
            .unwrap();
        drop(writer);
        assert!(matches!(
            owner.mail_retired_through(&project).await,
            Err(StorageError::InvalidStoredRecord)
        ));
        assert!(matches!(
            owner
                .advance_mail_retired_through(&project, 6.try_into().unwrap())
                .await,
            Err(StorageError::InvalidStoredRecord)
        ));
        owner.close().await.unwrap();
    }
}
