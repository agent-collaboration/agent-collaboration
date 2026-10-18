//! Holder-local migration history and bookkeeping never enter project capture.
use crate::ProjectStorageError;
use sqlx::{Connection, migrate::Migrator};

static BOOKKEEPING_MIGRATOR: Migrator = sqlx::migrate!("./project_bookkeeping_migrations");

pub(crate) async fn migrate_bookkeeping(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<(), ProjectStorageError> {
    let mut migrator = Migrator::with_migrations(BOOKKEEPING_MIGRATOR.iter().cloned().collect());
    migrator.dangerous_set_table_name("_replication_migrations");
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let result = migrator.run_direct(None, &mut *transaction, false).await;
    match result {
        Ok(()) => {
            transaction.commit().await?;
            Ok(())
        }
        Err(_) => {
            transaction.rollback().await?;
            Err(ProjectStorageError::InvalidSchema)
        }
    }
}
