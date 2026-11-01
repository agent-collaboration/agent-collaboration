//! The serialized physical owner; library scopes borrow its transaction connection.
use crate::approval_routes::StoredApprovalRoute;
use sqlx::Connection;
use sqlx_turso::TursoConnection;
use std::path::Path;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("interaction database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("interaction storage is unavailable")]
    StorageUnavailable,
    #[error("interaction schema is not owned or supported")]
    InvalidSchema,
    #[error("interaction storage contains an invalid stored record")]
    InvalidStoredRecord,
    #[error("interaction record is invalid or inconsistent")]
    InvalidRecord,
    #[error("push record id already exists")]
    PushAlreadyExists,
    #[error("push identity already belongs to different content")]
    PushContentConflict,
    #[error("push record was not found in this interaction database")]
    PushNotFound,
    #[error("push record delivery state does not permit this transition")]
    PushStateConflict,
    #[error("push record is not permitted for this caller")]
    PushNotPermitted,
    #[error("installation identities conflict with the immutable local directory")]
    InstallationPeerConflict,
}

pub struct InteractionStore {
    pub(crate) writer: Mutex<TursoConnection>,
}

impl InteractionStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let (connection, _) =
            crate::interaction_history::store::interaction_history_database::initialize_history(
                path.as_ref(),
            )
            .await?;
        Ok(Self {
            writer: Mutex::new(connection),
        })
    }

    pub async fn close(self) -> Result<(), StorageError> {
        self.writer.into_inner().close().await?;
        Ok(())
    }

    /// Imports the legacy file's full route set in one transaction.
    pub async fn import_approval_routes(
        &self,
        routes: &[StoredApprovalRoute],
        imported_at_ms: i64,
    ) -> Result<(), StorageError> {
        for route in routes {
            route.validate().map_err(|_| StorageError::InvalidRecord)?;
        }

        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let imported = async {
            for route in routes {
                sqlx::query::<sqlx_turso::Turso>(
                    "INSERT OR IGNORE INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,?,?)",
                )
                .bind(&route.thread_id)
                .bind(&route.route_json)
                .bind(imported_at_ms)
                .execute(&mut *transaction)
                .await?;
            }
            Ok::<_, StorageError>(())
        }
        .await;
        if let Err(error) = imported {
            transaction.rollback().await?;
            return Err(error);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Replaces a route and records registration as its latest use.
    pub async fn upsert_approval_route(
        &self,
        route: &StoredApprovalRoute,
        used_at_ms: i64,
    ) -> Result<(), StorageError> {
        route.validate().map_err(|_| StorageError::InvalidRecord)?;
        sqlx::query::<sqlx_turso::Turso>(
            "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,?,?) ON CONFLICT(thread_id) DO UPDATE SET route_json=excluded.route_json,last_used_at_ms=excluded.last_used_at_ms",
        )
        .bind(&route.thread_id)
        .bind(&route.route_json)
        .bind(used_at_ms)
        .execute(&mut *self.writer.lock().await)
        .await?;
        Ok(())
    }

    /// Reads one route and updates its last-use time in the same transaction.
    pub async fn approval_route_json(
        &self,
        thread_id: &str,
        used_at_ms: i64,
    ) -> Result<Option<String>, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let accessed = async {
            let route_json = sqlx::query_scalar::<sqlx_turso::Turso, String>(
                "SELECT route_json FROM approval_routes WHERE thread_id=?",
            )
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if route_json.is_some() {
                sqlx::query::<sqlx_turso::Turso>(
                    "UPDATE approval_routes SET last_used_at_ms=? WHERE thread_id=?",
                )
                .bind(used_at_ms)
                .bind(thread_id)
                .execute(&mut *transaction)
                .await?;
            }
            Ok::<_, sqlx::Error>(route_json)
        }
        .await;
        let route_json = match accessed {
            Ok(route_json) => route_json,
            Err(error) => {
                transaction.rollback().await?;
                return Err(StorageError::Database(error));
            }
        };
        transaction.commit().await?;
        Ok(route_json)
    }
}
